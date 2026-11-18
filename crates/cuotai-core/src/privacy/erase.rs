//! PRD 0052 RF5.2–RF5.4 — erase all Cuotai data, driven by the artifact
//! registry ([`super::artifacts`]).
//!
//! [`plan`] is the ordered inventory a frontend shows before asking; then
//! [`erase_all`] preflights the vault (service-wide search must work, or
//! nothing is deleted), removes every selected artifact and its
//! companions (rotated copy, SQLite sidecars), deletes every entry of the
//! `cuotai.oauth` and `cuotai.serve` services and removes the directories
//! left empty. A missing item is a success, so a second run is a no-op;
//! a failure is reported per id with a fixed class, never a path. The
//! accounts TOML and the adjacent `hooks.toml` are kept unless
//! [`EraseOptions::include_config`] is set; no external directory is ever
//! touched (symbolic links are removed, never followed).

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{artifacts, companions, Artifact, Erase, Kind, Roots};
use crate::diagnostics::redact::home_abbreviated;
use crate::text::Localized;

/// What [`erase_all`] removes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EraseOptions {
    pub roots: Roots,
    /// Also remove the accounts TOML and the adjacent `hooks.toml`.
    pub include_config: bool,
}

/// One line of the inventory shown before erasing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanItem {
    pub id: &'static str,
    /// Home-abbreviated path, or the vault service name.
    pub location: String,
    pub kind: Kind,
    /// Removed by this erase (`false`: listed as kept).
    pub erase: bool,
}

/// The ordered inventory for `options`: every registry artifact, the ones
/// kept marked so.
pub fn plan(options: &EraseOptions) -> Vec<PlanItem> {
    artifacts()
        .iter()
        .filter_map(|artifact| {
            let location = match artifact.kind {
                Kind::VaultService => artifact.path.to_string(),
                _ => home_abbreviated(&options.roots.path_of(artifact)?),
            };
            Some(PlanItem {
                id: artifact.id,
                location,
                kind: artifact.kind,
                erase: selected(artifact, options),
            })
        })
        .collect()
}

fn selected(artifact: &Artifact, options: &EraseOptions) -> bool {
    artifact.erase == Erase::Always || options.include_config
}

/// The word a person types to confirm erasing everything (the CLI prompt
/// and the desktop dialog), compared exactly after trimming.
pub fn confirmation_word() -> Localized {
    Localized::fixed("privacy.erase.word", "ERASE")
}

/// A person-facing name for an inventory item, when its location alone is
/// a technical name: the credential-vault services. Surfaces show it first
/// and the raw service name as secondary text.
pub fn item_label(id: &str) -> Option<Localized> {
    match id {
        "vault.oauth" => Some(Localized::fixed(
            "privacy.erase.label.vault_oauth",
            "Saved sign-ins, keys and channel addresses",
        )),
        "vault.serve" => Some(Localized::fixed(
            "privacy.erase.label.vault_serve",
            "Local server access token",
        )),
        _ => None,
    }
}

/// Whether `typed` is exactly the confirmation word of `locale` (spaces
/// around it ignored).
pub fn word_matches(typed: &str, locale: &str) -> bool {
    typed.trim() == confirmation_word().resolve(locale)
}

/// The typed word was not the confirmation word: nothing was erased.
pub fn word_mismatch() -> Localized {
    Localized::fixed(
        "privacy.erase.word_mismatch",
        "The confirmation word does not match; nothing was erased.",
    )
}

/// The native confirmation was cancelled or could not be shown.
pub fn declined() -> Localized {
    Localized::fixed(
        "privacy.erase.declined",
        "Not confirmed in the system dialog; nothing was erased.",
    )
}

/// The title of the desktop's native confirmation.
pub fn confirm_title() -> Localized {
    Localized::fixed("privacy.erase.confirm.title", "Erase all Cuotai data?")
}

/// The body of the desktop's native confirmation, from `plan`'s counts.
pub fn confirm_body(plan: &[PlanItem]) -> Localized {
    let erased = plan.iter().filter(|item| item.erase).count();
    let kept = plan.len() - erased;
    Localized::with_params(
        "privacy.erase.confirm.body",
        crate::text::params([("erased", erased.into()), ("kept", kept.into())]),
        "Cuotai will stop its background work and erase {erased} items: history, settings, logs, local cost data and the sign-ins it saved in the system credential vault. {kept} items are kept. This cannot be undone.",
    )
}

/// The accepting button of the desktop's native confirmation.
pub fn confirm_accept() -> Localized {
    Localized::fixed("privacy.erase.confirm.accept", "Erase everything")
}

/// The desktop's closing message after a complete erase.
pub fn done() -> Localized {
    Localized::fixed(
        "privacy.erase.done",
        "All Cuotai data was erased. Cuotai will now quit.",
    )
}

/// A vault operation failed (the class only: no backend text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VaultFailure;

/// The credential vault, as erasure sees it.
pub trait Vault {
    /// Whether every entry of `service` can be listed.
    fn can_search(&self, service: &str) -> bool;
    /// Delete every entry of `service`; how many went.
    fn delete_service(&self, service: &str) -> Result<usize, VaultFailure>;
}

/// The OS vault ([`crate::oauth::SystemTokenStore`]).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemVault;

impl Vault for SystemVault {
    fn can_search(&self, service: &str) -> bool {
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        {
            crate::oauth::SystemTokenStore::can_search(service)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            let _ = service;
            false
        }
    }

    fn delete_service(&self, service: &str) -> Result<usize, VaultFailure> {
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        {
            crate::oauth::SystemTokenStore::delete_service(service).map_err(|_| VaultFailure)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            let _ = service;
            Err(VaultFailure)
        }
    }
}

/// PRD 0050 `SEC-DEV-VAULT`: the plaintext development store as erasure
/// sees it (debug builds only; [`crate::oauth::StoreChoice`] builds it).
/// `cuotai.oauth` clears the token rows and the store's own vault index,
/// `cuotai.serve` the local-server token row; the database file itself is
/// kept, like the OS vault itself. It never reaches the system vault.
#[cfg(debug_assertions)]
pub struct DevelopmentVault {
    store: crate::oauth::SqliteDevStore,
    index: PathBuf,
}

#[cfg(debug_assertions)]
impl DevelopmentVault {
    /// Open the store at `path` with the development store's checks
    /// (regular private file, no symbolic link, a single link).
    pub(crate) fn open(path: &Path) -> Result<Self, Localized> {
        let store = crate::oauth::SqliteDevStore::open(path).map_err(|e| e.l10n())?;
        Ok(Self {
            store,
            index: crate::oauth::vault_index::development_path(path),
        })
    }

    /// Remove the store's vault index; a symbolic link is removed, never
    /// followed. Whether something went.
    fn remove_index(&self) -> Result<bool, VaultFailure> {
        match std::fs::symlink_metadata(&self.index) {
            Ok(meta) if meta.is_file() || meta.file_type().is_symlink() => {
                match std::fs::remove_file(&self.index) {
                    Ok(()) => Ok(true),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
                    Err(_) => Err(VaultFailure),
                }
            }
            // Not a file Cuotai wrote: left alone.
            Ok(_) => Err(VaultFailure),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(VaultFailure),
        }
    }
}

#[cfg(debug_assertions)]
impl Vault for DevelopmentVault {
    fn can_search(&self, service: &str) -> bool {
        matches!(service, "cuotai.oauth" | "cuotai.serve")
    }

    fn delete_service(&self, service: &str) -> Result<usize, VaultFailure> {
        match service {
            "cuotai.oauth" => {
                let rows = self.store.delete_all_tokens().map_err(|_| VaultFailure)?;
                Ok(rows + usize::from(self.remove_index()?))
            }
            "cuotai.serve" => self.store.serve_delete().map_err(|_| VaultFailure),
            _ => Err(VaultFailure),
        }
    }
}

/// How one artifact ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Something was removed.
    Removed,
    /// Nothing was there (a success).
    Absent,
    /// Not selected (the accounts TOML without `include_config`).
    Kept,
    /// Removal failed; retry.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemReport {
    pub id: &'static str,
    pub outcome: Outcome,
}

/// What [`erase_all`] did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EraseReport {
    pub items: Vec<ItemReport>,
}

impl EraseReport {
    /// Every selected artifact is gone.
    pub fn complete(&self) -> bool {
        self.items.iter().all(|i| i.outcome != Outcome::Failed)
    }

    pub fn failed(&self) -> Vec<&'static str> {
        self.items
            .iter()
            .filter(|i| i.outcome == Outcome::Failed)
            .map(|i| i.id)
            .collect()
    }

    pub fn count(&self, outcome: Outcome) -> usize {
        self.items.iter().filter(|i| i.outcome == outcome).count()
    }
}

/// Why nothing was erased.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EraseError {
    /// The vault cannot list Cuotai's services: deleting the files alone
    /// would leave credentials behind and look complete.
    VaultUnavailable,
}

impl EraseError {
    pub fn l10n(self) -> Localized {
        match self {
            Self::VaultUnavailable => Localized::fixed(
                "privacy.erase.vault_unavailable",
                "Nothing was erased: the system credential vault cannot list Cuotai's entries. Unlock it (on Linux, start a Secret Service such as GNOME Keyring in a D-Bus session) and try again.",
            ),
        }
    }
}

/// Erase every selected artifact (see the module documentation).
pub fn erase_all(options: &EraseOptions, vault: &dyn Vault) -> Result<EraseReport, EraseError> {
    let vaults: Vec<&Artifact> = artifacts()
        .iter()
        .filter(|a| a.kind == Kind::VaultService && selected(a, options))
        .collect();
    if vaults.iter().any(|a| !vault.can_search(a.path)) {
        return Err(EraseError::VaultUnavailable);
    }
    let mut items = Vec::new();
    for artifact in artifacts() {
        let outcome = if !selected(artifact, options) {
            Outcome::Kept
        } else if artifact.kind == Kind::VaultService {
            match vault.delete_service(artifact.path) {
                Ok(0) => Outcome::Absent,
                Ok(_) => Outcome::Removed,
                Err(VaultFailure) => Outcome::Failed,
            }
        } else {
            match options.roots.path_of(artifact) {
                Some(path) => remove(artifact, &path),
                None => Outcome::Absent,
            }
        };
        items.push(ItemReport {
            id: artifact.id,
            outcome,
        });
    }
    remove_empty_dirs(&options.roots);
    Ok(EraseReport { items })
}

fn remove(artifact: &Artifact, path: &Path) -> Outcome {
    let paths: Vec<PathBuf> = match artifact.kind {
        Kind::File => companions(artifact, path),
        _ => vec![path.to_path_buf()],
    };
    let mut removed = false;
    for path in paths {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let result = if meta.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            // A file or a symbolic link (the link itself, never its target).
            std::fs::remove_file(&path)
        };
        match result {
            Ok(()) => removed = true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Outcome::Failed,
        }
    }
    if removed {
        Outcome::Removed
    } else {
        Outcome::Absent
    }
}

/// Remove Cuotai's own directories once empty (`remove_dir` refuses a
/// directory that still holds anything, such as a kept TOML).
fn remove_empty_dirs(roots: &Roots) {
    let mut dirs: Vec<PathBuf> = artifacts()
        .iter()
        .filter_map(|a| match a.root {
            super::Root::LocalData | super::Root::Config => roots.path_of(a),
            _ => None,
        })
        .filter_map(|path| path.parent().map(Path::to_path_buf))
        .collect();
    dirs.extend(roots.local_data.clone());
    dirs.extend(roots.config.as_ref().map(|d| d.join("cuotai")));
    // Deepest first, so a parent empties after its children.
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    dirs.dedup();
    for dir in dirs {
        let _ = std::fs::remove_dir(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    #[test]
    fn vault_services_have_a_friendly_label_and_files_do_not() {
        for artifact in artifacts() {
            let label = item_label(artifact.id);
            assert_eq!(
                label.is_some(),
                artifact.kind == Kind::VaultService,
                "{}",
                artifact.id
            );
        }
        let label = item_label("vault.oauth").unwrap();
        assert_ne!(label.resolve("pt-BR"), label.resolve("en-US"));
        assert!(!label.resolve("en-US").contains("cuotai.oauth"));
    }

    #[test]
    fn the_confirmation_word_is_exact_and_localized() {
        assert!(word_matches("ERASE", "en-US"));
        assert!(word_matches("  ERASE\n", "en-US"));
        assert!(!word_matches("erase", "en-US"));
        assert!(!word_matches("", "en-US"));
        assert!(word_matches("APAGAR", "pt-BR"));
        assert!(!word_matches("ERASE", "pt-BR"));
        let plan = plan(&EraseOptions {
            roots: Roots::default(),
            include_config: false,
        });
        let body = confirm_body(&plan).resolve("en-US");
        assert!(body.contains(&plan.iter().filter(|i| i.erase).count().to_string()));
    }

    /// A vault with entries per service, that can be made to fail.
    #[derive(Default)]
    struct FakeVault {
        entries: RefCell<BTreeMap<String, usize>>,
        searchable: bool,
        fail_delete: bool,
    }

    impl Vault for FakeVault {
        fn can_search(&self, _service: &str) -> bool {
            self.searchable
        }

        fn delete_service(&self, service: &str) -> Result<usize, VaultFailure> {
            if self.fail_delete {
                return Err(VaultFailure);
            }
            Ok(self.entries.borrow_mut().remove(service).unwrap_or(0))
        }
    }

    /// A complete artifact tree under a temporary directory.
    fn tree() -> (tempfile::TempDir, EraseOptions) {
        let dir = tempfile::tempdir().unwrap();
        let roots = Roots {
            local_data: Some(dir.path().join("local")),
            config: Some(dir.path().join("config")),
            accounts: Some(dir.path().join("config").join("cuotai").join("config.toml")),
            // The macOS widget copies, in a home of their own (never the
            // real one).
            mac_home: Some(dir.path().join("home")),
            // PRD 0033 re-validation M-4: the fixed metered folder, apart
            // from the moved data folder, which keeps a legacy copy; an
            // erase removes both.
            metered: Some(dir.path().join("os-local").join("cuotai").join("metered")),
            // PRD 0040 re-validation V-08: the fixed feed folder, likewise.
            feed: Some(dir.path().join("os-local").join("cuotai").join("feed")),
        };
        for artifact in artifacts() {
            let Some(path) = roots.path_of(artifact) else {
                continue;
            };
            match artifact.kind {
                Kind::Directory => {
                    std::fs::create_dir_all(path.join("sub")).unwrap();
                    std::fs::write(path.join("sub").join("f"), b"x").unwrap();
                }
                _ => {
                    for companion in companions(artifact, &path) {
                        std::fs::create_dir_all(companion.parent().unwrap()).unwrap();
                        std::fs::write(&companion, b"data").unwrap();
                    }
                }
            }
        }
        let options = EraseOptions {
            roots,
            include_config: false,
        };
        (dir, options)
    }

    fn vault() -> FakeVault {
        FakeVault {
            entries: RefCell::new(
                [
                    ("cuotai.oauth".to_string(), 3),
                    ("cuotai.serve".to_string(), 1),
                ]
                .into(),
            ),
            searchable: true,
            fail_delete: false,
        }
    }

    fn files_left(root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                if entry.path().is_dir() {
                    stack.push(entry.path());
                } else {
                    out.push(entry.path());
                }
            }
        }
        out
    }

    #[test]
    fn a_complete_erase_leaves_only_the_unselected_config_and_is_idempotent() {
        let (dir, options) = tree();
        let vault = vault();
        let report = erase_all(&options, &vault).unwrap();
        assert!(report.complete(), "{report:?}");
        assert_eq!(
            report.count(Outcome::Kept),
            2,
            "accounts TOML and hooks.toml"
        );
        assert!(
            vault.entries.borrow().is_empty(),
            "both vault services emptied"
        );
        let left = files_left(dir.path());
        let accounts = options.roots.accounts.clone().unwrap();
        let hooks = accounts.with_file_name("hooks.toml");
        // The kept files keep their own copies (`.bak`) too.
        assert_eq!(left.len(), 4, "{left:?}");
        assert!(left.contains(&accounts) && left.contains(&hooks));
        assert!(left
            .iter()
            .all(|path| path.starts_with(accounts.parent().unwrap())));
        assert!(
            !dir.path().join("local").join("cuotai").exists(),
            "emptied directories go"
        );
        // A second run finds nothing and still succeeds.
        let again = erase_all(&options, &vault).unwrap();
        assert!(again.complete());
        assert_eq!(again.count(Outcome::Removed), 0);
    }

    /// PRD 0051 RF3.3 / PRD 0003 COMP-02: the backups and set-aside copies
    /// Cuotai keeps of a file go with it.
    #[test]
    fn backups_and_set_aside_copies_are_erased_with_their_file() {
        let (_dir, options) = tree();
        let at = |file: &str| {
            artifacts()
                .iter()
                .find(|artifact| artifact.path.ends_with(file))
                .and_then(|artifact| options.roots.path_of(artifact))
                .unwrap()
        };
        let sibling = |path: PathBuf, suffix: &str| {
            let mut name = path.into_os_string();
            name.push(suffix);
            PathBuf::from(name)
        };
        let extra = [
            sibling(at("notified.json"), ".corrupt-20261107T120000Z"),
            sibling(at("notified.json"), ".corrupt-20261107T120000Z-1"),
            sibling(at("history.sqlite3"), ".bak"),
        ];
        for path in &extra {
            std::fs::write(path, b"x").unwrap();
        }
        let desktop = sibling(at("desktop.json"), ".bak.3");
        std::fs::write(&desktop, b"x").unwrap();
        erase_all(&options, &vault()).unwrap();
        for path in extra.iter().chain([&desktop]) {
            assert!(!path.exists(), "{} left", path.display());
        }
    }

    /// PRD 0032 RF4.2: a ledger lock held at the moment of the erase and a
    /// `.stale-` leftover of a crash go too, and the data folder with them.
    #[test]
    fn the_ledger_lock_and_its_stale_leftover_are_erased() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("local");
        let options = EraseOptions {
            roots: Roots {
                local_data: Some(data.clone()),
                ..Roots::default()
            },
            include_config: false,
        };
        std::fs::create_dir_all(&data).unwrap();
        let lock = data.join("notified.lock");
        let leftover = data.join("notified.lock.stale-1-0");
        std::fs::write(&lock, "1:0").unwrap();
        std::fs::write(&leftover, "1:0").unwrap();
        let report = erase_all(&options, &vault()).unwrap();
        assert!(report.complete(), "{report:?}");
        assert!(!lock.exists() && !leftover.exists());
        assert!(!data.exists(), "the emptied data folder goes");
    }

    #[test]
    fn include_config_also_removes_the_toml_and_hooks_file() {
        let (dir, mut options) = tree();
        options.include_config = true;
        assert!(plan(&options).iter().all(|i| i.erase));
        let report = erase_all(&options, &vault()).unwrap();
        assert!(report.complete());
        assert_eq!(report.count(Outcome::Kept), 0);
        assert!(files_left(dir.path()).is_empty());
    }

    #[test]
    fn an_unsearchable_vault_refuses_before_deleting_any_file() {
        let (dir, options) = tree();
        let before = files_left(dir.path()).len();
        let vault = FakeVault {
            searchable: false,
            ..vault()
        };
        assert_eq!(
            erase_all(&options, &vault),
            Err(EraseError::VaultUnavailable)
        );
        assert_eq!(files_left(dir.path()).len(), before);
    }

    #[test]
    fn a_partial_failure_is_reported_and_a_retry_completes() {
        let (_dir, options) = tree();
        let failing = FakeVault {
            fail_delete: true,
            ..vault()
        };
        let report = erase_all(&options, &failing).unwrap();
        assert!(!report.complete());
        assert_eq!(report.failed(), vec!["vault.oauth", "vault.serve"]);
        let retry = erase_all(&options, &vault()).unwrap();
        assert!(retry.complete());
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_is_removed_without_touching_its_target() {
        let (dir, options) = tree();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep"), b"x").unwrap();
        let extensions = options
            .roots
            .path_of(super::super::artifact("extensions.tree").unwrap())
            .unwrap();
        std::fs::remove_dir_all(&extensions).unwrap();
        std::os::unix::fs::symlink(&outside, &extensions).unwrap();
        assert!(erase_all(&options, &vault()).unwrap().complete());
        assert!(outside.join("keep").exists());
        assert!(std::fs::symlink_metadata(&extensions).is_err());
    }

    /// A system vault stand-in that counts how often its factory ran.
    struct CountingVault;

    impl Vault for CountingVault {
        fn can_search(&self, _: &str) -> bool {
            true
        }

        fn delete_service(&self, _: &str) -> Result<usize, VaultFailure> {
            Ok(1)
        }
    }

    #[cfg(debug_assertions)]
    fn os_vault_reached() -> crate::oauth::EraseVault {
        panic!("the OS vault was reached")
    }

    #[cfg(debug_assertions)]
    fn token(value: &str) -> crate::oauth::OAuthToken {
        crate::oauth::OAuthToken {
            access_token: value.into(),
            refresh_token: Some("refresh".into()),
            expires_at: None,
            metadata: Default::default(),
        }
    }

    /// PRD 0050 `SEC-DEV-VAULT` (S-04): erase-all on the development store
    /// clears that store's rows and index, keeps its file, and never calls
    /// the system factory.
    // security-invariant: SEC-DEV-VAULT
    #[cfg(debug_assertions)]
    #[test]
    fn the_development_store_is_erased_and_the_system_vault_is_never_reached() {
        use crate::oauth::{SqliteDevStore, StoreChoice, TokenStore};
        let (dir, options) = tree();
        let database = dir.path().join("dev").join("cuotai-dev.sqlite3");
        std::fs::create_dir_all(database.parent().unwrap()).unwrap();
        let store = SqliteDevStore::open(&database).unwrap();
        store.save("first", &token("a")).unwrap();
        store.save("second", &token("b")).unwrap();
        #[cfg(feature = "serve")]
        store.serve_save("cuotai_0123456789abcdef").unwrap();
        drop(store);
        let index = crate::oauth::vault_index::development_path(&database);
        std::fs::write(&index, b"{}").unwrap();

        let choice = StoreChoice::Development(database.clone());
        let vault = choice.erase_vault_with(os_vault_reached).unwrap();
        let report = erase_all(&options, &*vault).unwrap();
        assert!(report.complete(), "{report:?}");
        assert_eq!(outcome_of(&report, "vault.oauth"), Outcome::Removed);
        #[cfg(feature = "serve")]
        assert_eq!(outcome_of(&report, "vault.serve"), Outcome::Removed);
        assert!(!index.exists(), "the store's index goes");
        assert!(database.exists(), "the database file is kept");
        let store = SqliteDevStore::open(&database).unwrap();
        assert!(store.load("first").unwrap().is_none());
        assert!(store.load("second").unwrap().is_none());
        #[cfg(feature = "serve")]
        assert!(store.serve_load().unwrap().is_none());
        drop(store);

        // A second run finds nothing and still succeeds.
        let vault = choice.erase_vault_with(os_vault_reached).unwrap();
        let again = erase_all(&options, &*vault).unwrap();
        assert!(again.complete());
        assert_eq!(outcome_of(&again, "vault.oauth"), Outcome::Absent);
        assert_eq!(outcome_of(&again, "vault.serve"), Outcome::Absent);
    }

    #[cfg(debug_assertions)]
    fn outcome_of(report: &EraseReport, id: &str) -> Outcome {
        report.items.iter().find(|i| i.id == id).unwrap().outcome
    }

    /// PRD 0050 `SEC-DEV-VAULT`: the system choice takes its vault from the
    /// system factory, exactly once.
    // security-invariant: SEC-DEV-VAULT
    #[test]
    fn the_system_choice_uses_the_system_factory_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = AtomicUsize::new(0);
        let vault = crate::oauth::StoreChoice::from_flag(None)
            .erase_vault_with(|| {
                calls.fetch_add(1, Ordering::SeqCst);
                Box::new(CountingVault)
            })
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(vault.can_search("cuotai.oauth"));
    }

    /// PRD 0050 `SEC-DEV-VAULT` / `SEC-DEV-FILE`: a development store that
    /// is shared with other users or reached through a symbolic link is
    /// refused, and the system factory is not called instead.
    // security-invariant: SEC-DEV-VAULT
    #[cfg(all(unix, debug_assertions))]
    #[test]
    fn a_development_vault_refuses_a_shared_or_linked_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let shared = dir.path().join("shared.sqlite3");
        std::fs::write(&shared, b"").unwrap();
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o644)).unwrap();
        let link = dir.path().join("link.sqlite3");
        let private = dir.path().join("private.sqlite3");
        crate::oauth::SqliteDevStore::open(&private).unwrap();
        std::os::unix::fs::symlink(&private, &link).unwrap();
        for path in [shared, link] {
            let choice = crate::oauth::StoreChoice::Development(path);
            let Err(text) = choice.erase_vault_with(os_vault_reached) else {
                panic!("a shared or linked file must be refused");
            };
            assert_eq!(text.id, "oauth_error.store.sqlite_dev_unavailable");
        }
    }

    #[test]
    fn the_plan_lists_every_artifact_in_registry_order() {
        let (_dir, options) = tree();
        let plan = plan(&options);
        assert_eq!(plan.len(), artifacts().len());
        assert_eq!(plan.iter().filter(|i| !i.erase).count(), 2);
        let vault = plan.iter().find(|i| i.id == "vault.oauth").unwrap();
        assert_eq!(vault.location, "cuotai.oauth");
    }
}
