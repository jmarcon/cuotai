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
//!
//! PRD 0051 re-validation S-01: Cuotai deletes only what it created. A data
//! root that is not [`crate::data_dir::owned`] is left as it is (every item
//! found there is [`Outcome::Foreign`]); in an owned one a directory item
//! loses only its Cuotai-shaped entries, and the ownership marker goes
//! last, only when nothing else is left in the root.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{artifacts, companions, Artifact, Erase, Kind, Root, Roots};
use crate::diagnostics::redact::home_abbreviated;
pub use crate::presence::Program;
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
    /// Why a kept item is kept, when the accounts-file box does not decide
    /// it (a data folder Cuotai did not create).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Localized>,
    /// PRD 0052 S-01/S-03: a [`Kind::AppManaged`] item, which only the
    /// desktop app erases ([`erase_all`] leaves it alone; the CLI lists it
    /// apart with [`app_only`]).
    pub app_only: bool,
}

/// The id of a plan line for a data folder an erase leaves alone.
pub const FOREIGN_FOLDER_ID: &str = "local_data.not_cuotai";

/// The ordered inventory for `options`: every registry artifact, the ones
/// kept marked so; then the data items of every other data root
/// ([`Roots::other_local_data`]), and one kept line per data folder Cuotai
/// did not create ([`Roots::kept_local_data`]).
pub fn plan(options: &EraseOptions) -> Vec<PlanItem> {
    let roots = &options.roots;
    let item = |artifact: &'static Artifact, roots: &Roots| {
        let location = match artifact.kind {
            Kind::VaultService => artifact.path.to_string(),
            Kind::AppManaged => app_location(artifact.id),
            _ => home_abbreviated(&roots.path_of(artifact)?),
        };
        Some(PlanItem {
            id: artifact.id,
            location,
            kind: artifact.kind,
            erase: selected(artifact, options),
            reason: None,
            app_only: artifact.kind == Kind::AppManaged,
        })
    };
    let mut items: Vec<PlanItem> = artifacts()
        .iter()
        .filter_map(|artifact| item(artifact, roots))
        .collect();
    for root in &roots.other_local_data {
        let view = roots.with_local_data(root);
        items.extend(
            artifacts()
                .iter()
                .filter(|artifact| artifact.root == Root::LocalData)
                .filter_map(|artifact| item(artifact, &view)),
        );
    }
    items.extend(roots.kept_local_data.iter().map(|root| PlanItem {
        id: FOREIGN_FOLDER_ID,
        location: home_abbreviated(root),
        kind: Kind::Directory,
        erase: false,
        reason: Some(not_cuotai_folder()),
        app_only: false,
    }));
    items
}

/// Why a data folder an erase found is kept.
pub fn not_cuotai_folder() -> Localized {
    Localized::fixed(
        "privacy.erase.not_cuotai_folder",
        "Kept: this folder was not created by Cuotai.",
    )
}

/// Where an app-managed item lives on this operating system, in plain text.
fn app_location(id: &str) -> String {
    match id {
        "webview.storage" => "dev.cuotai.desktop (WebKit / WebView2)".to_string(),
        "autostart.entry" => if cfg!(target_os = "macos") {
            "~/Library/LaunchAgents/<app>.plist"
        } else if cfg!(windows) {
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run"
        } else {
            "~/.config/autostart/<app>.desktop"
        }
        .to_string(),
        "capture.profiles" => "<temp>/cuotai-session-capture-*".to_string(),
        _ => String::new(),
    }
}

fn selected(artifact: &Artifact, options: &EraseOptions) -> bool {
    artifact.erase == Erase::Always || options.include_config
}

/// The word a person types to confirm erasing everything (the CLI prompt
/// and the desktop dialog), compared exactly after trimming, in Unicode NFC.
pub fn confirmation_word() -> Localized {
    Localized::fixed("privacy.erase.word", "ERASE")
}

/// A person-facing name for an inventory item, when its location alone is
/// a technical name: the credential-vault services and the items only the
/// desktop app erases. Surfaces show it first
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
        "webview.storage" => Some(Localized::fixed(
            "privacy.erase.label.webview_storage",
            "The app's window storage",
        )),
        "autostart.entry" => Some(Localized::fixed(
            "privacy.erase.label.autostart",
            "Open at login",
        )),
        "capture.profiles" => Some(Localized::fixed(
            "privacy.erase.label.capture_profiles",
            "Leftover web-session capture profiles",
        )),
        _ => None,
    }
}

/// PRD 0052 S-01/S-03: the line the CLI prints for the items only the
/// desktop app can erase (`items`: their names, already resolved).
pub fn app_only(items: &str) -> Localized {
    Localized::with_param(
        "privacy.erase.app_only",
        "items",
        items,
        "Erase in the desktop app: {items}",
    )
}

/// Whether `typed` is exactly the confirmation word of `locale` (spaces
/// around it ignored). Both sides are compared in Unicode NFC, so a
/// keyboard or input method that sends an accented letter as a base letter
/// plus a combining mark still matches; the English word is never accepted
/// in place of another language's.
pub fn word_matches(typed: &str, locale: &str) -> bool {
    use unicode_normalization::UnicodeNormalization;
    let word = confirmation_word().resolve(locale);
    typed.trim().nfc().eq(word.nfc())
}

/// The typed word was not the confirmation word: nothing was erased.
pub fn word_mismatch() -> Localized {
    Localized::fixed(
        "privacy.erase.word_mismatch",
        "The confirmation word does not match; nothing was erased.",
    )
}

/// PRD 0052 S-04: how a running program is named in [`running`], in
/// `locale`: the two terminal commands literally (never translated), the
/// app and the dashboard in words.
pub fn program_name(program: Program, locale: &str) -> String {
    match program {
        Program::Desktop => Localized::fixed(
            "privacy.erase.running.desktop",
            "The Cuotai app (also in the menu bar or system tray)",
        )
        .resolve(locale),
        Program::Serve => "cuotai --serve".to_string(),
        Program::Watch => "cuotai --watch".to_string(),
        Program::Dashboard => Localized::fixed(
            "privacy.erase.running.dashboard",
            "The Cuotai terminal dashboard",
        )
        .resolve(locale),
    }
}

/// PRD 0052 S-04: nothing was erased because other Cuotai programs run;
/// one line per program, named in `locale` (the English fallback names
/// them in English).
pub fn running(programs: &[Program], locale: &str) -> Localized {
    let lines = |locale: &str| -> String {
        programs
            .iter()
            .map(|program| format!("\n  - {}", program_name(*program, locale)))
            .collect()
    };
    let mut text = Localized::with_param(
        "privacy.erase.running",
        "programs",
        lines("en-US"),
        "Nothing was erased while Cuotai is running. Quit these first, then run the erase again:{programs}",
    );
    text.params
        .insert("programs".to_string(), lines(locale).into());
    text
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

/// PRD 0051 re-validation S-01: shown under the closing message when
/// `count` items were left in place because Cuotai did not create them.
pub fn foreign(count: usize) -> Localized {
    Localized::plural(
        "privacy.erase.foreign",
        crate::text::params([("count", count.into())]),
        "{count} item was left in place because it is in a folder, or holds files, that Cuotai did not create.",
        "{count} items were left in place because they are in a folder, or hold files, that Cuotai did not create.",
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
    /// `<database>.oauth-locks` (PRD 0052 re-validation S-05).
    locks: PathBuf,
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
            locks: crate::oauth::development_locks_dir(path),
        })
    }

    /// Remove the store's refresh-lock folder: its `<key>.lock` files, then
    /// the folder once empty; a link is removed, never followed, and a
    /// folder that keeps other entries stays. Whether something went.
    fn remove_locks(&self) -> Result<bool, VaultFailure> {
        let meta = match std::fs::symlink_metadata(&self.locks) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(VaultFailure),
        };
        if !meta.is_dir() {
            return if meta.file_type().is_symlink() {
                std::fs::remove_file(&self.locks).map_err(|_| VaultFailure)?;
                Ok(true)
            } else {
                Ok(false)
            };
        }
        let entries = std::fs::read_dir(&self.locks).map_err(|_| VaultFailure)?;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let is_lock = std::fs::symlink_metadata(entry.path()).is_ok_and(|m| m.is_file())
                && lock_name(&name.to_string_lossy());
            if is_lock {
                std::fs::remove_file(entry.path()).map_err(|_| VaultFailure)?;
            }
        }
        Ok(std::fs::remove_dir(&self.locks).is_ok())
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
                Ok(rows + usize::from(self.remove_index()?) + usize::from(self.remove_locks()?))
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
    /// Not selected (the accounts TOML without `include_config`), or an
    /// app-managed item [`erase_all`] leaves to the desktop app.
    Kept,
    /// Removal failed; retry.
    Failed,
    /// Left in place: it is in a folder Cuotai did not create, or holds
    /// files Cuotai did not create (not a failure).
    Foreign,
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

/// The ownership marker's registry id: removed last, by
/// [`remove_empty_dirs`].
const MARKER_ID: &str = "data.marker";

/// What erasing knows about the roots before anything is removed.
struct Scope {
    /// The data root is Cuotai's own ([`crate::data_dir::owned`]).
    local_owned: bool,
    /// Cuotai's hook files sit beside `hooks/work` in the data root.
    hooks_beside_work: bool,
}

impl Scope {
    fn of(roots: &Roots) -> Self {
        let local = roots.local_data.as_deref();
        let regular = |path: PathBuf| std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file());
        Self {
            local_owned: local.is_some_and(crate::data_dir::owned),
            hooks_beside_work: local.is_some_and(|root| {
                let hooks = root.join("hooks");
                ["consent.json", crate::hooks::CLAIMS_FILE_NAME, "runs.log"]
                    .into_iter()
                    .any(|name| regular(hooks.join(name)))
            }),
        }
    }
}

/// The desktop settings' registry id: removed after every data root, and
/// kept while any data item failed, so a retry still finds the folder they
/// name (PRD 0052 re-validation S-02).
const DESKTOP_ID: &str = "desktop.settings";

/// Erase every selected artifact (see the module documentation), in every
/// data root ([`Roots::local_data`] and [`Roots::other_local_data`]); an
/// item's outcomes across roots merge (a failure wins, then something left
/// in place, then a removal).
pub fn erase_all(options: &EraseOptions, vault: &dyn Vault) -> Result<EraseReport, EraseError> {
    let vaults: Vec<&Artifact> = artifacts()
        .iter()
        .filter(|a| a.kind == Kind::VaultService && selected(a, options))
        .collect();
    if vaults.iter().any(|a| !vault.can_search(a.path)) {
        return Err(EraseError::VaultUnavailable);
    }
    let roots = &options.roots;
    let scope = Scope::of(roots);
    let mut items = Vec::new();
    for artifact in artifacts() {
        let outcome = if !selected(artifact, options) || artifact.kind == Kind::AppManaged {
            Outcome::Kept
        } else if artifact.kind == Kind::VaultService {
            match vault.delete_service(artifact.path) {
                Ok(0) => Outcome::Absent,
                Ok(_) => Outcome::Removed,
                Err(VaultFailure) => Outcome::Failed,
            }
        } else if artifact.id == DESKTOP_ID || artifact.id == MARKER_ID {
            // Settled below.
            Outcome::Absent
        } else {
            erase_one(artifact, roots, &scope)
        };
        items.push(ItemReport {
            id: artifact.id,
            outcome,
        });
    }
    let mut markers = vec![(roots.clone(), scope.local_owned)];
    for root in &roots.other_local_data {
        let view = roots.with_local_data(root);
        let scope = Scope::of(&view);
        for artifact in artifacts()
            .iter()
            .filter(|a| a.root == Root::LocalData && a.id != MARKER_ID && selected(a, options))
        {
            let outcome = erase_one(artifact, &view, &scope);
            merge_into(&mut items, artifact.id, outcome);
        }
        markers.push((view, scope.local_owned));
    }
    if let Some(desktop) = super::artifact(DESKTOP_ID).filter(|a| selected(a, options)) {
        let data_failed = items.iter().any(|item| {
            item.outcome == Outcome::Failed
                && super::artifact(item.id).is_some_and(|a| a.root == Root::LocalData)
        });
        let outcome = match roots.path_of(desktop) {
            None => Outcome::Absent,
            Some(path) if data_failed => match std::fs::symlink_metadata(&path) {
                Ok(_) => Outcome::Failed,
                Err(_) => Outcome::Absent,
            },
            Some(path) => remove(desktop, &path, &scope),
        };
        set_outcome(&mut items, DESKTOP_ID, outcome);
    }
    let selected_marker = super::artifact(MARKER_ID).is_some_and(|a| selected(a, options));
    let mut marker = None;
    for (view, owned) in &markers {
        if let Some(outcome) = remove_empty_local(view, *owned) {
            marker = Some(marker.map_or(outcome, |m| merge(m, outcome)));
        }
    }
    remove_empty_config(roots);
    if let (true, Some(outcome)) = (selected_marker, marker) {
        set_outcome(&mut items, MARKER_ID, outcome);
    }
    Ok(EraseReport { items })
}

/// Erase one file or directory item under `roots`.
fn erase_one(artifact: &Artifact, roots: &Roots, scope: &Scope) -> Outcome {
    match roots.path_of(artifact) {
        Some(path) if artifact.root == Root::LocalData && !scope.local_owned => untouched(&path),
        Some(path) => remove(artifact, &path, scope),
        None => Outcome::Absent,
    }
}

/// Two outcomes of one item in different roots: a failure wins, then
/// something left in place, then a removal, then absence.
fn merge(a: Outcome, b: Outcome) -> Outcome {
    let rank = |o: Outcome| match o {
        Outcome::Failed => 4,
        Outcome::Foreign => 3,
        Outcome::Removed => 2,
        Outcome::Kept => 1,
        Outcome::Absent => 0,
    };
    if rank(b) > rank(a) {
        b
    } else {
        a
    }
}

fn merge_into(items: &mut [ItemReport], id: &str, outcome: Outcome) {
    if let Some(item) = items.iter_mut().find(|item| item.id == id) {
        item.outcome = merge(item.outcome, outcome);
    }
}

fn set_outcome(items: &mut [ItemReport], id: &str, outcome: Outcome) {
    if let Some(item) = items.iter_mut().find(|item| item.id == id) {
        item.outcome = outcome;
    }
}

/// An item in a folder Cuotai does not own: never touched.
fn untouched(path: &Path) -> Outcome {
    if std::fs::symlink_metadata(path).is_ok() {
        Outcome::Foreign
    } else {
        Outcome::Absent
    }
}

fn remove(artifact: &Artifact, path: &Path, scope: &Scope) -> Outcome {
    if artifact.kind == Kind::Directory {
        return remove_tree(artifact, path, scope);
    }
    let mut removed = false;
    for path in companions(artifact, path) {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            // Not a file Cuotai wrote under that name.
            return Outcome::Foreign;
        }
        // A file or a symbolic link (the link itself, never its target).
        match std::fs::remove_file(&path) {
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

/// What a directory item may lose.
enum Shape {
    /// All of it (Cuotai's beyond doubt).
    Whole,
    /// Only the entries the predicate accepts (name, metadata of the entry,
    /// links not followed).
    Entries(fn(&Path, &str, &std::fs::Metadata) -> bool),
    /// Nothing but an empty folder.
    EmptyOnly,
}

fn shape(artifact: &Artifact, path: &Path, scope: &Scope) -> Shape {
    match artifact.id {
        "extensions.tree" => Shape::Entries(|path, _, meta| {
            meta.is_dir() && crate::extensions::looks_like_extension(path)
        }),
        "hooks.work" if scope.hooks_beside_work => Shape::Whole,
        "canary.records" if crate::canary::Store::erasable(path) => Shape::Whole,
        "feed.tree" | "feed.tree_legacy" => Shape::Entries(|_, name, meta| {
            meta.is_dir() && crate::feed::FeedSource::from_slug(name).is_some()
        }),
        "metered.records" | "metered.records_legacy" => {
            Shape::Entries(|_, name, meta| meta.is_file() && metered_name(name))
        }
        "oauth.locks" => Shape::Entries(|_, name, meta| meta.is_file() && lock_name(name)),
        // PRD 0052 S-04: `<program>-<pid>.lock` files only.
        "process.presence" => {
            Shape::Entries(|_, name, meta| meta.is_file() && crate::presence::is_lock_name(name))
        }
        _ => Shape::EmptyOnly,
    }
}

/// An OAuth refresh lock (`<key>.lock`, a key of letters, digits, `-`, `_`
/// and `.` as the vault keys are).
fn lock_name(name: &str) -> bool {
    name.strip_suffix(".lock").is_some_and(|key| {
        !key.is_empty()
            && key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    })
}

/// A metered record (`<64 hex>.json`), its lock (`.<record>.lock`) or a
/// temporary file of its writer (`.<record>.<random>.tmp`).
fn metered_name(name: &str) -> bool {
    let record = |name: &str| {
        name.strip_suffix(".json").is_some_and(|digest| {
            digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit())
        })
    };
    if record(name) {
        return true;
    }
    let Some(inner) = name.strip_prefix('.') else {
        return false;
    };
    if let Some(record_name) = inner.strip_suffix(".lock") {
        return record(record_name);
    }
    inner
        .strip_suffix(".tmp")
        .and_then(|rest| rest.get(..69))
        .is_some_and(|prefix| record(prefix) && inner.as_bytes().get(69) == Some(&b'.'))
}

/// Remove a directory item by its [`Shape`]: a symbolic link is removed
/// itself (never followed); a directory that keeps entries Cuotai did not
/// create is [`Outcome::Foreign`].
fn remove_tree(artifact: &Artifact, path: &Path, scope: &Scope) -> Outcome {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Outcome::Absent;
    };
    if meta.file_type().is_symlink() {
        return match std::fs::remove_file(path) {
            Ok(()) => Outcome::Removed,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Outcome::Absent,
            Err(_) => Outcome::Failed,
        };
    }
    if !meta.is_dir() {
        return Outcome::Foreign;
    }
    let accept = match shape(artifact, path, scope) {
        Shape::Whole => {
            return match std::fs::remove_dir_all(path) {
                Ok(()) => Outcome::Removed,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Outcome::Absent,
                Err(_) => Outcome::Failed,
            }
        }
        Shape::Entries(accept) => Some(accept),
        Shape::EmptyOnly => None,
    };
    let Ok(entries) = std::fs::read_dir(path) else {
        return Outcome::Failed;
    };
    let mut kept = false;
    for entry in entries.flatten() {
        let entry_path = entry.path();
        let Ok(entry_meta) = std::fs::symlink_metadata(&entry_path) else {
            continue;
        };
        let name = entry.file_name();
        if !accept.is_some_and(|accept| accept(&entry_path, &name.to_string_lossy(), &entry_meta)) {
            kept = true;
            continue;
        }
        let result = if entry_meta.is_dir() {
            std::fs::remove_dir_all(&entry_path)
        } else {
            std::fs::remove_file(&entry_path)
        };
        match result {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Outcome::Failed,
        }
    }
    if kept {
        return Outcome::Foreign;
    }
    match std::fs::remove_dir(path) {
        Ok(()) => Outcome::Removed,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Outcome::Absent,
        Err(_) => Outcome::Failed,
    }
}

/// Remove the data root's own directories once empty (`remove_dir`
/// refuses a directory that still holds anything); in a data root Cuotai
/// does not own, nothing. Before the root itself goes, its ownership marker
/// is removed, only when it is the last entry left. The marker's outcome is
/// returned (`None` without a data root).
fn remove_empty_local(roots: &Roots, owned: bool) -> Option<Outcome> {
    let root = roots.local_data.as_deref()?;
    let marker = root.join(crate::data_dir::MARKER);
    if !owned {
        return Some(untouched(&marker));
    }
    let existed = std::fs::symlink_metadata(&marker).is_ok();
    let mut dirs: Vec<PathBuf> = artifacts()
        .iter()
        .filter(|a| a.root == Root::LocalData)
        .filter_map(|a| roots.path_of(a))
        .filter_map(|path| path.parent().map(Path::to_path_buf))
        .collect();
    dirs.push(root.to_path_buf());
    remove_dirs(dirs, |dir| {
        if dir == root {
            remove_marker_if_last(dir);
        }
    });
    Some(
        match (existed, std::fs::symlink_metadata(&marker).is_ok()) {
            (false, _) => Outcome::Absent,
            (true, false) => Outcome::Removed,
            (true, true) => Outcome::Foreign,
        },
    )
}

/// Remove the configuration folder's own directories once empty (a kept
/// TOML keeps its folder).
fn remove_empty_config(roots: &Roots) {
    let mut dirs: Vec<PathBuf> = artifacts()
        .iter()
        .filter(|a| a.root == Root::Config)
        .filter_map(|a| roots.path_of(a))
        .filter_map(|path| path.parent().map(Path::to_path_buf))
        .collect();
    dirs.extend(roots.config.as_ref().map(|d| d.join("cuotai")));
    remove_dirs(dirs, |_| {});
}

/// `remove_dir` each of `dirs`, deepest first so a parent empties after its
/// children, calling `before` with each first.
fn remove_dirs(mut dirs: Vec<PathBuf>, before: impl Fn(&Path)) {
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    dirs.dedup();
    for dir in dirs {
        before(&dir);
        let _ = std::fs::remove_dir(dir);
    }
}

/// Remove `<root>/.cuotai-data` when it is the root's only entry.
fn remove_marker_if_last(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let names: Vec<_> = entries
        .flatten()
        .map(|entry| entry.file_name())
        .take(2)
        .collect();
    if names.len() == 1 && names[0] == crate::data_dir::MARKER {
        let _ = std::fs::remove_file(root.join(crate::data_dir::MARKER));
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
                matches!(artifact.kind, Kind::VaultService | Kind::AppManaged),
                "{}",
                artifact.id
            );
        }
        let label = item_label("vault.oauth").unwrap();
        assert_ne!(label.resolve("pt-BR"), label.resolve("en-US"));
        assert!(!label.resolve("en-US").contains("cuotai.oauth"));
    }

    /// PRD 0052 re-validation V-03: a word typed as a base letter plus a
    /// combining mark (an input method's decomposed form) matches the
    /// catalog's composed word in every language; the English word still
    /// does not stand in for another language's.
    #[test]
    fn the_confirmation_word_matches_in_any_unicode_form() {
        use unicode_normalization::UnicodeNormalization;
        assert!(word_matches("XO\u{0301}A", "vi"));
        assert!(word_matches("SI\u{0307}L", "tr"));
        assert!(word_matches("LO\u{0308}SCHEN", "de"));
        // Hangul typed as conjoining jamo.
        assert!(word_matches(
            "\u{1109}\u{1161}\u{11A8}\u{110C}\u{1166}",
            "ko"
        ));
        for locale in [
            "en-US", "pt-BR", "es", "it", "de", "fr", "ru", "ko", "ja", "zh-CN", "hi-IN", "id-ID",
            "tr", "vi",
        ] {
            let word = confirmation_word().resolve(locale);
            let decomposed: String = word.nfd().collect();
            assert!(word_matches(&decomposed, locale), "{locale}: {word}");
            assert!(word_matches(&format!(" {decomposed} "), locale), "{locale}");
            if locale != "en-US" {
                assert!(!word_matches("ERASE", locale), "{locale} accepts ERASE");
            }
        }
        assert!(
            !word_matches("XOA", "vi"),
            "a missing accent must not match"
        );
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

    /// PRD 0052 S-04: the refusal names each running program on a line of
    /// its own, the commands literally and the rest in the language.
    #[test]
    fn the_running_refusal_names_each_program_in_the_language() {
        let programs = [Program::Desktop, Program::Serve, Program::Dashboard];
        let english = running(&programs, "en-US");
        assert_eq!(english.id, "privacy.erase.running");
        let text = english.resolve("en-US");
        assert!(text.starts_with("Nothing was erased while Cuotai is running."));
        assert_eq!(text.lines().count(), 4, "{text}");
        assert!(text.contains("\n  - cuotai --serve"));
        let portuguese = running(&programs, "pt-BR");
        let text = portuguese.resolve("pt-BR");
        assert!(text.contains("cuotai --serve") && text.contains("O app Cuotai"));
        assert!(
            !portuguese.fallback.contains("O app Cuotai"),
            "English fallback"
        );
        assert_eq!(program_name(Program::Watch, "ja"), "cuotai --watch");
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
            ..Roots::default()
        };
        for artifact in artifacts() {
            let Some(path) = roots.path_of(artifact) else {
                continue;
            };
            match artifact.kind {
                Kind::Directory => cuotai_entries(artifact.id, &path),
                _ if artifact.id == MARKER_ID => {
                    crate::data_dir::mark_for_tests(path.parent().unwrap()).unwrap();
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

    const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// Entries of the shape Cuotai writes in the directory item `id`.
    fn cuotai_entries(id: &str, path: &Path) {
        let write = |rel: &str, bytes: &[u8]| {
            let file = path.join(rel);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, bytes).unwrap();
        };
        match id {
            "extensions.tree" => write(
                "demo/manifest.toml",
                b"id = \"demo\"\nname = \"Demo\"\nversion = \"1\"\nschema = 1\ncommand = [\"./run\"]\ninterval_seconds = 60\n",
            ),
            "feed.tree" | "feed.tree_legacy" => write("claude_code/session.json", b"{}"),
            "metered.records" | "metered.records_legacy" => {
                write(&format!("{DIGEST}.json"), b"{}");
                write(&format!(".{DIGEST}.json.lock"), b"");
                write(&format!(".{DIGEST}.json.a1b2.tmp"), b"");
            }
            "canary.records" => {
                write(crate::canary::MARKER, b"");
                write("run-1.json", b"{}");
            }
            "oauth.locks" => write("0f8e5a3c-1b2d-4c6e-8a9b-112233445566.lock", b""),
            "process.presence" => write("serve-123.lock", b""),
            _ => write("sub/f", b"x"),
        }
    }

    /// Someone else's entries in every directory item, and a file of theirs
    /// beside Cuotai's in the data root.
    fn foreign_entries(roots: &Roots) -> Vec<PathBuf> {
        let mut planted = Vec::new();
        for artifact in artifacts().iter().filter(|a| a.kind == Kind::Directory) {
            let Some(path) = roots.path_of(artifact) else {
                continue;
            };
            let file = path.join("their-notes").join("notes.txt");
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, b"mine").unwrap();
            planted.push(file);
        }
        let beside = roots.local_data.as_ref().unwrap().join("their-file.txt");
        std::fs::write(&beside, b"mine").unwrap();
        planted.push(beside);
        planted
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
            2 + app_managed(),
            "accounts TOML, hooks.toml and the app-managed items"
        );
        assert!(
            vault.entries.borrow().is_empty(),
            "both vault services emptied"
        );
        let left = files_left(dir.path());
        let accounts = options.roots.accounts.clone().unwrap();
        let hooks = accounts.with_file_name("hooks.toml");
        // The kept files keep their own copies (`.bak`) and locks too.
        assert_eq!(left.len(), 6, "{left:?}");
        assert!(left.contains(&accounts) && left.contains(&hooks));
        assert!(left
            .iter()
            .all(|path| path.starts_with(accounts.parent().unwrap())));
        assert!(
            !dir.path().join("local").exists(),
            "emptied directories go, the marker last"
        );
        assert_eq!(report.count(Outcome::Foreign), 0, "{report:?}");
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
        crate::data_dir::mark_for_tests(&data).unwrap();
        let lock = data.join("notified.lock");
        let leftover = data.join("notified.lock.stale-1-0");
        std::fs::write(&lock, "1:0").unwrap();
        std::fs::write(&leftover, "1:0").unwrap();
        let report = erase_all(&options, &vault()).unwrap();
        assert!(report.complete(), "{report:?}");
        assert!(!lock.exists() && !leftover.exists());
        assert!(!data.exists(), "the emptied data folder goes");
    }

    /// PRD 0051 re-validation S-01: a data root Cuotai does not own (no
    /// marker, not the OS default) is never touched: every item found there
    /// is `Foreign`, every byte stays, and the erase is still complete.
    #[test]
    fn an_unowned_data_root_is_left_byte_for_byte() {
        let (dir, options) = tree();
        let local = options.roots.local_data.clone().unwrap();
        std::fs::remove_file(local.join(crate::data_dir::MARKER)).unwrap();
        foreign_entries(&options.roots);
        let snapshot = |root: &Path| {
            let mut files: Vec<(PathBuf, Vec<u8>)> = files_left(root)
                .into_iter()
                .map(|path| {
                    let bytes = std::fs::read(&path).unwrap();
                    (path, bytes)
                })
                .collect();
            files.sort();
            files
        };
        let before = snapshot(&local);
        let report = erase_all(&options, &vault()).unwrap();
        assert!(report.complete(), "{report:?}");
        assert_eq!(snapshot(&local), before, "nothing in the root changed");
        for id in [
            "extensions.tree",
            "canary.records",
            "metered.records_legacy",
            "feed.tree_legacy",
            "hooks.work",
            "notifications.ledger",
            "history.db",
        ] {
            assert_eq!(
                report.items.iter().find(|i| i.id == id).unwrap().outcome,
                Outcome::Foreign,
                "{id}"
            );
        }
        assert_eq!(
            report
                .items
                .iter()
                .find(|i| i.id == MARKER_ID)
                .unwrap()
                .outcome,
            Outcome::Absent
        );
        // The fixed per-user folders and the config are still erased.
        let fixed = options.roots.metered.clone().unwrap();
        assert!(!fixed.join(format!("{DIGEST}.json")).exists());
        assert!(!dir
            .path()
            .join("config")
            .join("cuotai")
            .join("desktop.json")
            .exists());
    }

    /// In an owned root, a directory item loses only Cuotai's entries: the
    /// foreign ones stay, the directory is `Foreign`, and the marker stays
    /// with the root because the root is not empty.
    #[test]
    fn an_owned_root_keeps_foreign_entries_and_its_marker() {
        let (_dir, options) = tree();
        let local = options.roots.local_data.clone().unwrap();
        let planted = foreign_entries(&options.roots);
        let report = erase_all(&options, &vault()).unwrap();
        assert!(report.complete(), "{report:?}");
        // `hooks/work` (beside Cuotai's hook files) and a marked canary
        // folder are Cuotai's as a whole and go with what is in them.
        let whole = [local.join("hooks").join("work"), local.join("canary")];
        for path in planted
            .iter()
            .filter(|p| !whole.iter().any(|w| p.starts_with(w)))
        {
            assert_eq!(std::fs::read(path).unwrap(), b"mine", "{}", path.display());
        }
        let outcome = |id: &str| report.items.iter().find(|i| i.id == id).unwrap().outcome;
        assert_eq!(outcome("hooks.work"), Outcome::Removed);
        assert_eq!(outcome("canary.records"), Outcome::Removed);
        for id in [
            "extensions.tree",
            "feed.tree",
            "metered.records",
            "metered.records_legacy",
        ] {
            assert_eq!(outcome(id), Outcome::Foreign, "{id}");
        }
        assert_eq!(outcome("history.db"), Outcome::Removed);
        let extensions = local.join("extensions");
        assert!(!extensions.join("demo").exists(), "Cuotai's extension went");
        let fixed = options.roots.metered.clone().unwrap();
        assert!(!fixed.join(format!("{DIGEST}.json")).exists());
        assert!(!fixed.join(format!(".{DIGEST}.json.lock")).exists());
        assert!(!options
            .roots
            .feed
            .clone()
            .unwrap()
            .join("claude_code")
            .exists());
        assert!(
            local.join(crate::data_dir::MARKER).is_file(),
            "the marker stays"
        );
        assert_eq!(outcome(MARKER_ID), Outcome::Foreign);
    }

    /// Every data item of a root, Cuotai-shaped, and its marker.
    fn populate_local(view: &Roots) {
        for artifact in artifacts().iter().filter(|a| a.root == Root::LocalData) {
            let Some(path) = view.path_of(artifact) else {
                continue;
            };
            match artifact.kind {
                Kind::Directory => cuotai_entries(artifact.id, &path),
                _ if artifact.id == MARKER_ID => {
                    crate::data_dir::mark_for_tests(path.parent().unwrap()).unwrap();
                }
                _ => {
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(&path, b"data").unwrap();
                }
            }
        }
    }

    /// A tree whose desktop settings choose `chosen` (populated and marked
    /// when `marked`), plus a populated stand-in for the OS default folder.
    fn tree_with_other_roots(marked: bool) -> (tempfile::TempDir, EraseOptions, PathBuf, PathBuf) {
        let (dir, mut options) = tree();
        let chosen = dir.path().join("chosen");
        let default = dir.path().join("os-default");
        populate_local(&options.roots.with_local_data(&chosen));
        populate_local(&options.roots.with_local_data(&default));
        if !marked {
            std::fs::remove_file(chosen.join(crate::data_dir::MARKER)).unwrap();
            std::fs::write(chosen.join("taxes.pdf"), b"mine").unwrap();
        }
        let desktop = options
            .roots
            .path_of(super::super::artifact(DESKTOP_ID).unwrap())
            .unwrap();
        std::fs::write(
            &desktop,
            serde_json::json!({ "data_dir": chosen }).to_string(),
        )
        .unwrap();
        let config = options.roots.config.clone().unwrap();
        options.roots = options.roots.with_erase_roots(
            [
                crate::data_dir::desktop_choice(&config),
                Some(default.clone()),
            ]
            .into_iter()
            .flatten(),
        );
        (dir, options, chosen, default)
    }

    /// PRD 0052 re-validation S-02: the desktop's chosen folder and data
    /// left in the default one are erased with the data root in use, and
    /// the desktop settings that name the choice go last.
    #[test]
    fn the_desktop_choice_and_the_default_folder_are_both_erased_and_desktop_json_goes_last() {
        let (dir, options, chosen, default) = tree_with_other_roots(true);
        assert_eq!(
            options.roots.other_local_data,
            vec![chosen.clone(), default.clone()]
        );
        let shown = plan(&options);
        for root in [&chosen, &default] {
            let history = home_abbreviated(&root.join("history").join("history.sqlite3"));
            assert!(
                shown
                    .iter()
                    .any(|i| i.id == "history.db" && i.location == history && i.erase),
                "{history}"
            );
        }
        let vault = vault();
        let report = erase_all(&options, &vault).unwrap();
        assert!(report.complete(), "{report:?}");
        assert_eq!(report.count(Outcome::Foreign), 0, "{report:?}");
        assert!(!chosen.exists() && !default.exists(), "both roots emptied");
        assert!(!dir
            .path()
            .join("config")
            .join("cuotai")
            .join("desktop.json")
            .exists());
        let again = erase_all(&options, &vault).unwrap();
        assert!(again.complete());
        assert_eq!(again.count(Outcome::Removed), 0, "{again:?}");
    }

    /// A data item that cannot be removed in the chosen folder keeps
    /// `desktop.json` (so a retry still finds the folder) and is a partial
    /// erase; once fixed, a retry completes.
    #[cfg(unix)]
    #[test]
    fn a_failure_in_the_chosen_folder_keeps_desktop_json() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, options, chosen, _default) = tree_with_other_roots(true);
        let locked = chosen.join("history");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
        let report = erase_all(&options, &vault()).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!report.complete());
        let failed = report.failed();
        assert!(failed.contains(&"history.db"), "{failed:?}");
        assert!(failed.contains(&DESKTOP_ID), "{failed:?}");
        let desktop = dir
            .path()
            .join("config")
            .join("cuotai")
            .join("desktop.json");
        assert!(desktop.exists(), "kept for the retry");
        let retry = erase_all(&options, &vault()).unwrap();
        assert!(retry.complete(), "{retry:?}");
        assert!(!desktop.exists() && !chosen.exists());
    }

    /// A chosen folder without Cuotai's marker is listed as kept, with the
    /// reason, and never emptied.
    #[test]
    fn a_chosen_folder_without_the_cuotai_marker_is_never_emptied() {
        let (_dir, options, chosen, default) = tree_with_other_roots(false);
        assert_eq!(options.roots.kept_local_data, vec![chosen.clone()]);
        assert_eq!(options.roots.other_local_data, vec![default.clone()]);
        let shown = plan(&options);
        let kept = shown.iter().find(|i| i.id == FOREIGN_FOLDER_ID).unwrap();
        assert!(!kept.erase);
        assert_eq!(kept.location, home_abbreviated(&chosen));
        assert_eq!(
            kept.reason.as_ref().map(|r| r.id.as_ref()),
            Some("privacy.erase.not_cuotai_folder")
        );
        let report = erase_all(&options, &vault()).unwrap();
        assert!(report.complete(), "{report:?}");
        assert_eq!(std::fs::read(chosen.join("taxes.pdf")).unwrap(), b"mine");
        assert!(chosen.join("history").join("history.sqlite3").exists());
        assert!(!default.exists());
    }

    /// PRD 0052 re-validation S-05: writers' dot-locks, crashed writers'
    /// temporaries and the OAuth refresh locks go with their files, so the
    /// configuration folder and the data root are removed too.
    #[test]
    fn dot_locks_temporaries_and_oauth_locks_go_and_the_roots_are_removed() {
        let (dir, mut options) = tree();
        options.include_config = true;
        let config = dir.path().join("config").join("cuotai");
        let local = options.roots.local_data.clone().unwrap();
        for path in [
            config.join(".desktop.json.lock"),
            config.join(".desktop.json.Ab12Cd.tmp"),
            local.join(".notified.json.lock"),
            local.join(".widget.json.x9Yz.tmp"),
            local.join("cost").join(".settings.json.lock"),
        ] {
            std::fs::write(&path, b"").unwrap();
        }
        assert!(local.join("oauth-locks").is_dir());
        let report = erase_all(&options, &vault()).unwrap();
        assert!(report.complete(), "{report:?}");
        assert_eq!(report.count(Outcome::Foreign), 0, "{report:?}");
        assert!(!config.exists(), "{:?}", files_left(&config));
        assert!(!local.exists(), "{:?}", files_left(&local));
    }

    #[test]
    fn lock_names_are_vault_keys_only() {
        assert!(lock_name("0f8e5a3c-1b2d-4c6e-8a9b-112233445566.lock"));
        assert!(!lock_name(".lock"));
        assert!(!lock_name("notes.txt"));
        assert!(!lock_name("a b.lock"));
    }

    #[test]
    fn metered_names_are_records_locks_and_temporaries_only() {
        assert!(metered_name(&format!("{DIGEST}.json")));
        assert!(metered_name(&format!(".{DIGEST}.json.lock")));
        assert!(metered_name(&format!(".{DIGEST}.json.x9Yz.tmp")));
        assert!(!metered_name("notes.json"));
        assert!(!metered_name(&format!("{DIGEST}.txt")));
        assert!(!metered_name(&format!(".{DIGEST}.jsonx.tmp")));
        assert!(!metered_name(&format!("{}.json", &DIGEST[1..])));
    }

    #[test]
    fn include_config_also_removes_the_toml_and_hooks_file() {
        let (dir, mut options) = tree();
        options.include_config = true;
        assert!(plan(&options).iter().all(|i| i.erase));
        let report = erase_all(&options, &vault()).unwrap();
        assert!(report.complete());
        assert_eq!(report.count(Outcome::Kept), app_managed());
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
        // PRD 0052 re-validation S-05: the store's refresh locks go too.
        let locks = crate::oauth::development_locks_dir(&database);
        std::fs::create_dir_all(&locks).unwrap();
        std::fs::write(locks.join("first.lock"), b"").unwrap();

        let choice = StoreChoice::Development(database.clone());
        let vault = choice.erase_vault_with(os_vault_reached).unwrap();
        let report = erase_all(&options, &*vault).unwrap();
        assert!(report.complete(), "{report:?}");
        assert_eq!(outcome_of(&report, "vault.oauth"), Outcome::Removed);
        #[cfg(feature = "serve")]
        assert_eq!(outcome_of(&report, "vault.serve"), Outcome::Removed);
        assert!(!index.exists(), "the store's index goes");
        assert!(!locks.exists(), "the store's lock folder goes");
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

    fn app_managed() -> usize {
        artifacts()
            .iter()
            .filter(|a| a.kind == Kind::AppManaged)
            .count()
    }

    /// PRD 0052 S-01/S-03: the web storage, the start-at-login entry and
    /// capture profiles are listed (to be erased, by the app only) and
    /// `erase_all` reports them kept without touching anything.
    #[test]
    fn app_managed_artifacts_are_never_touched_by_erase_all_and_are_listed_as_app_only() {
        let (_dir, options) = tree();
        let plan = plan(&options);
        let ids: Vec<_> = plan.iter().filter(|i| i.app_only).map(|i| i.id).collect();
        assert_eq!(
            ids,
            vec!["webview.storage", "autostart.entry", "capture.profiles"]
        );
        for item in plan.iter().filter(|i| i.app_only) {
            assert!(item.erase, "{}: the app erases it", item.id);
            assert_eq!(item.kind, Kind::AppManaged);
            assert!(!item.location.is_empty() && !item.location.contains('`'));
            assert!(item_label(item.id).is_some());
        }
        assert!(plan
            .iter()
            .filter(|i| !i.app_only)
            .all(|i| i.kind != Kind::AppManaged));
        let report = erase_all(&options, &vault()).unwrap();
        for id in ids {
            let item = report.items.iter().find(|i| i.id == id).unwrap();
            assert_eq!(item.outcome, Outcome::Kept, "{id}");
        }
        let text = app_only("Open at login").resolve("pt-BR");
        assert!(text.contains("Open at login") && !text.starts_with("Erase"));
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
