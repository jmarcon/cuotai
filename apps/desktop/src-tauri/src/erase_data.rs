//! PRD 0052 RF5.2/RF5.3: Settings › Data & privacy › **Erase all Cuotai
//! data**.
//!
//! The webview shows the core's ordered inventory ([`erase::plan`]) and
//! sends only the typed word and the "also delete the accounts file"
//! choice. Rust re-checks the word against the core's word in the desktop
//! language, preflights the vault, asks once more in a native dialog it
//! builds from core texts, then stops every writer (and locks
//! `desktop.json` and the app log for the rest of the process) **before**
//! [`erase::erase_all`] runs. A complete erase ends the process right
//! after a closing message, so no worker can recreate a file; a partial
//! one returns the failed ids (never a path) with the writers still
//! stopped, and may be retried. One erase runs at a time.
//!
//! PRD 0052 S-04: the app holds its [`presence`] lock for its whole life
//! ([`hold_presence`]) and refuses to erase while another Cuotai program
//! (`cuotai --serve`, `cuotai --watch`, the terminal dashboard or a second
//! app) runs and could write its files back.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cuotai_core::execution_review;
use cuotai_core::presence::{self, Presence, Program};
use cuotai_core::privacy::erase::{self, EraseOptions, Outcome, PlanItem, Vault};
use cuotai_core::privacy::{artifacts, Erase, Kind, Roots};
use cuotai_core::text::Localized;
use serde::Serialize;

use crate::native_confirm::{NativeConfirmer, Question};

/// Set once the writers stopped: `desktop.json`, configuration loading and
/// the app log stay closed until the process ends.
static LOCKED: AtomicBool = AtomicBool::new(false);

/// Whether an erase stopped the writers in this process.
pub fn locked() -> bool {
    LOCKED.load(Ordering::SeqCst)
}

/// This process's presence lock, held from the start and released last by
/// [`AppWriters::stop`] (so the erase can remove the `running` folder, also
/// where a held file cannot be deleted).
static PRESENCE: std::sync::Mutex<Option<Presence>> = std::sync::Mutex::new(None);

/// PRD 0052 S-04: hold the app's presence until the process ends or an
/// erase stops the writers (best effort, never fatal).
pub fn hold_presence() {
    let held = presence::hold(Program::Desktop);
    *PRESENCE.lock().unwrap_or_else(|e| e.into_inner()) = held;
}

fn release_presence() {
    drop(PRESENCE.lock().unwrap_or_else(|e| e.into_inner()).take());
}

/// One erase at a time; a second request answers `execution.pending`.
static RUNNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// What the dialog lists before anything is asked.
#[derive(Debug, Serialize)]
pub struct PlanView {
    /// The word to type, in the desktop language.
    word: String,
    /// Every artifact in the core's order; `erase: false` is kept unless
    /// the accounts-file box is ticked.
    items: Vec<PlanItemView>,
}

/// One inventory line: the core item plus, for a credential-vault service,
/// its friendly name in the desktop language (the raw service name stays in
/// `location`, shown as secondary text).
#[derive(Debug, Serialize)]
pub struct PlanItemView {
    #[serde(flatten)]
    item: PlanItem,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
}

/// The inventory over `roots`, the accounts TOML marked as kept.
pub fn plan_view(roots: Roots, locale: &str) -> PlanView {
    PlanView {
        word: erase::confirmation_word().resolve(locale),
        items: erase::plan(&EraseOptions {
            roots,
            include_config: false,
        })
        .into_iter()
        .map(|item| PlanItemView {
            label: erase::item_label(item.id)
                .or_else(|| item.reason.clone())
                .map(|label| label.resolve(locale)),
            item,
        })
        .collect(),
    }
}

/// How an erase ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EraseResult {
    pub complete: bool,
    pub removed: usize,
    pub absent: usize,
    /// Logical ids that could not be removed (no path).
    pub failed: Vec<&'static str>,
    /// Items left in place because Cuotai did not create them (PRD 0051
    /// re-validation S-01); not a failure.
    pub foreign: usize,
}

/// Everything that writes Cuotai files, stopped before erasing.
pub trait Writers: Send + Sync {
    fn stop<'a>(&'a self) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}

/// The app's writers: the backend's loops, server, hooks, widgets and
/// history, the local cost scan, then `desktop.json`, configuration loading
/// and the app log (locked for good).
pub struct AppWriters {
    pub backend: Arc<crate::backend::Backend>,
    pub cost: Arc<crate::local_cost::LocalCost>,
}

impl Writers for AppWriters {
    fn stop<'a>(&'a self) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            LOCKED.store(true, Ordering::SeqCst);
            self.backend.stop_for_erase().await;
            // Let a scan in progress finish its write, then end the worker.
            for _ in 0..100 {
                if !self.cost.scanning() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            self.cost.stop();
            cuotai_core::diagnostics::logging::suspend();
            release_presence();
        })
    }
}

/// The other Cuotai programs running now ([`presence::running`] skips
/// this process); the tests pass a fake.
pub type Running = dyn Fn() -> Vec<Program> + Sync;

/// The erase transaction (see the module documentation), over any
/// confirmer, writers, vault and running-program check so the tests drive
/// it with fakes.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    typed: &str,
    include_config: bool,
    roots: Roots,
    locale: &str,
    confirmer: &dyn NativeConfirmer,
    writers: &dyn Writers,
    vault: &(dyn Vault + Sync),
    running: &Running,
) -> Result<EraseResult, Localized> {
    let _running = RUNNING
        .try_lock()
        .map_err(|_| execution_review::pending())?;
    if !erase::word_matches(typed, locale) {
        return Err(erase::word_mismatch());
    }
    // PRD 0052 S-04: another Cuotai program could write its files back;
    // nothing is asked, stopped or erased.
    let others = running();
    if !others.is_empty() {
        return Err(erase::running(&others, locale));
    }
    let options = EraseOptions {
        roots,
        include_config,
    };
    // The vault must list Cuotai's services before anything stops: deleting
    // only the files would look complete and leave credentials behind.
    let searchable = artifacts()
        .iter()
        .filter(|a| a.kind == Kind::VaultService && (a.erase == Erase::Always || include_config))
        .all(|a| vault.can_search(a.path));
    if !searchable {
        return Err(erase::EraseError::VaultUnavailable.l10n());
    }
    let plan = erase::plan(&options);
    let question = Question {
        title: erase::confirm_title().resolve(locale),
        body: erase::confirm_body(&plan).resolve(locale),
        accept: erase::confirm_accept().resolve(locale),
        cancel: execution_review::cancel().resolve(locale),
    };
    if !confirmer.ask(question).await {
        return Err(erase::declined());
    }
    writers.stop().await;
    let report = erase::erase_all(&options, vault).map_err(|e| e.l10n())?;
    Ok(EraseResult {
        complete: report.complete(),
        removed: report.count(Outcome::Removed),
        absent: report.count(Outcome::Absent),
        failed: report.failed(),
        foreign: report.count(Outcome::Foreign),
    })
}

/// After a complete erase: a closing native message (with how many items
/// were left in place because Cuotai did not create them), then the process
/// ends (the run loop's exit hooks find `desktop.json` locked). The exit
/// happens even when the message cannot be shown.
pub fn quit(app: &tauri::AppHandle, locale: &str, foreign: usize) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    let handle = app.clone();
    app.dialog()
        .message(closing_message(locale, foreign))
        .title(erase::confirm_title().resolve(locale).trim_end_matches('?'))
        .kind(MessageDialogKind::Info)
        .show(move |_| handle.exit(0));
}

/// The closing message: the core's "done" text, and under it the
/// left-in-place note when there is one.
fn closing_message(locale: &str, foreign: usize) -> String {
    let done = erase::done().resolve(locale);
    if foreign == 0 {
        done
    } else {
        format!("{done}\n\n{}", erase::foreign(foreign).resolve(locale))
    }
}

/// The roots of an erase for the loaded accounts TOML (or none): the data
/// folder in use plus data left in the OS default one (PRD 0052
/// re-validation S-02, [`Roots::for_erase`]).
pub fn roots(accounts: Option<PathBuf>) -> Roots {
    Roots::for_erase(accounts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;

    /// One erase transaction at a time in this module's tests (the gate is
    /// process-wide).
    static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct Answer(bool, Mutex<Vec<Question>>);

    impl NativeConfirmer for Answer {
        fn ask<'a>(&'a self, q: Question) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
            self.1.lock().unwrap().push(q);
            let answer = self.0;
            Box::pin(async move { answer })
        }
    }

    /// Records the stop and checks that nothing was erased yet.
    struct FakeWriters {
        stopped: AtomicUsize,
        watched: PathBuf,
        present_at_stop: AtomicBool,
    }

    impl Writers for FakeWriters {
        fn stop<'a>(&'a self) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
            Box::pin(async move {
                self.stopped.fetch_add(1, Ordering::SeqCst);
                self.present_at_stop
                    .store(self.watched.exists(), Ordering::SeqCst);
            })
        }
    }

    struct FakeVault {
        searchable: bool,
        searched: AtomicUsize,
        deleted: Mutex<Vec<String>>,
    }

    impl Vault for FakeVault {
        fn can_search(&self, _: &str) -> bool {
            self.searched.fetch_add(1, Ordering::SeqCst);
            self.searchable
        }
        fn delete_service(&self, service: &str) -> Result<usize, erase::VaultFailure> {
            self.deleted.lock().unwrap().push(service.to_owned());
            Ok(1)
        }
    }

    /// No other Cuotai program runs.
    fn none() -> Vec<Program> {
        Vec::new()
    }

    fn vault(searchable: bool) -> FakeVault {
        FakeVault {
            searchable,
            searched: AtomicUsize::new(0),
            deleted: Mutex::new(Vec::new()),
        }
    }

    /// A data root with a history database and an accounts file beside it.
    fn tree() -> (tempfile::TempDir, Roots, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let history = data.join("history").join("history.sqlite3");
        std::fs::create_dir_all(history.parent().unwrap()).unwrap();
        std::fs::write(&history, b"x").unwrap();
        cuotai_core::data_dir::mark_for_tests(&data).unwrap();
        let accounts = dir.path().join("conf").join("cuotai.toml");
        std::fs::create_dir_all(accounts.parent().unwrap()).unwrap();
        std::fs::write(&accounts, b"").unwrap();
        let roots = Roots {
            local_data: Some(data),
            config: Some(dir.path().join("config")),
            accounts: Some(accounts.clone()),
            mac_home: None,
            metered: None,
            feed: None,
            ..Roots::default()
        };
        (dir, roots, history, accounts)
    }

    fn writers(watched: &std::path::Path) -> FakeWriters {
        FakeWriters {
            stopped: AtomicUsize::new(0),
            watched: watched.to_path_buf(),
            present_at_stop: AtomicBool::new(false),
        }
    }

    #[tokio::test]
    async fn a_wrong_word_touches_nothing_and_asks_nothing() {
        let _serial = SERIAL.lock().await;
        let (_dir, roots, history, _) = tree();
        let confirmer = Answer(true, Mutex::new(Vec::new()));
        let writers = writers(&history);
        let vault = vault(true);
        for typed in ["erase", "", "ERASE!", "APAGAR"] {
            let error = run(
                typed,
                false,
                roots.clone(),
                "en-US",
                &confirmer,
                &writers,
                &vault,
                &none,
            )
            .await
            .unwrap_err();
            assert_eq!(error, erase::word_mismatch());
        }
        assert!(history.exists());
        assert_eq!(writers.stopped.load(Ordering::SeqCst), 0);
        assert!(confirmer.1.lock().unwrap().is_empty());
        assert_eq!(vault.searched.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn the_word_is_the_one_of_the_desktop_language() {
        let _serial = SERIAL.lock().await;
        let (_dir, roots, history, _) = tree();
        let confirmer = Answer(true, Mutex::new(Vec::new()));
        let writers = writers(&history);
        let result = run(
            " APAGAR ",
            false,
            roots,
            "pt-BR",
            &confirmer,
            &writers,
            &vault(true),
            &none,
        )
        .await
        .unwrap();
        assert!(result.complete);
        assert!(!history.exists());
        assert_eq!(plan_view(Roots::default(), "pt-BR").word, "APAGAR");
    }

    #[tokio::test]
    async fn writers_stop_before_anything_is_erased_and_the_config_is_kept() {
        let _serial = SERIAL.lock().await;
        let (_dir, roots, history, accounts) = tree();
        let confirmer = Answer(true, Mutex::new(Vec::new()));
        let writers = writers(&history);
        let vault = vault(true);
        let result = run(
            "ERASE", false, roots, "en-US", &confirmer, &writers, &vault, &none,
        )
        .await
        .unwrap();
        assert_eq!(writers.stopped.load(Ordering::SeqCst), 1);
        assert!(
            writers.present_at_stop.load(Ordering::SeqCst),
            "the writers stop while the data is still there"
        );
        assert!(result.complete && result.failed.is_empty());
        assert!(result.removed >= 1);
        assert!(!history.exists());
        assert!(accounts.exists(), "kept without include_config");
        assert_eq!(
            *vault.deleted.lock().unwrap(),
            vec!["cuotai.oauth".to_owned(), "cuotai.serve".to_owned()]
        );
        let asked = confirmer.1.lock().unwrap();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].title, "Erase all Cuotai data?");
        assert_eq!(asked[0].accept, "Erase everything");
    }

    /// PRD 0050 `SEC-DEV-VAULT` (S-04): a desktop started on the development
    /// store erases that store's sign-ins and server token, and the system
    /// vault factory is never called.
    // security-invariant: SEC-DEV-VAULT
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn the_desktop_erases_the_store_it_started_with() {
        let _serial = SERIAL.lock().await;
        let (dir, roots, history, _) = tree();
        let database = dir.path().join("cuotai-dev.sqlite3");
        let choice = cuotai_core::oauth::StoreChoice::Development(database.clone());
        let token = cuotai_core::oauth::OAuthToken {
            access_token: "placeholder".into(),
            refresh_token: None,
            expires_at: None,
            metadata: Default::default(),
        };
        choice
            .session()
            .unwrap()
            .token_store()
            .save("first", &token)
            .unwrap();
        let serve = choice
            .serve_vault_with(|| panic!("the OS vault was reached"))
            .unwrap();
        serve
            .save(&cuotai_core::serve::Token::generate().unwrap())
            .unwrap();
        let vault = choice
            .erase_vault_with(|| panic!("the OS vault was reached"))
            .unwrap();
        let confirmer = Answer(true, Mutex::new(Vec::new()));
        let writers = writers(&history);
        let result = run(
            "ERASE", false, roots, "en-US", &confirmer, &writers, &*vault, &none,
        )
        .await
        .unwrap();
        assert!(result.complete && result.failed.is_empty());
        assert!(!history.exists());
        assert!(database.exists(), "the store's file is kept");
        let store = choice.session().unwrap().token_store();
        assert!(
            store.load("first").unwrap().is_none(),
            "its sign-ins are gone"
        );
        assert!(serve.load().unwrap().is_none(), "its server token is gone");
    }

    #[tokio::test]
    async fn include_config_also_removes_the_accounts_file() {
        let _serial = SERIAL.lock().await;
        let (_dir, roots, history, accounts) = tree();
        let confirmer = Answer(true, Mutex::new(Vec::new()));
        let writers = writers(&history);
        let result = run(
            "ERASE",
            true,
            roots,
            "en-US",
            &confirmer,
            &writers,
            &vault(true),
            &none,
        )
        .await
        .unwrap();
        assert!(result.complete);
        assert!(!accounts.exists());
    }

    #[tokio::test]
    async fn cancel_or_an_unsearchable_vault_stops_nothing() {
        let _serial = SERIAL.lock().await;
        let (_dir, roots, history, _) = tree();
        let writers = writers(&history);
        let cancel = Answer(false, Mutex::new(Vec::new()));
        let error = run(
            "ERASE",
            false,
            roots.clone(),
            "en-US",
            &cancel,
            &writers,
            &vault(true),
            &none,
        )
        .await
        .unwrap_err();
        assert_eq!(error, erase::declined());
        let accept = Answer(true, Mutex::new(Vec::new()));
        let error = run(
            "ERASE",
            false,
            roots,
            "en-US",
            &accept,
            &writers,
            &vault(false),
            &none,
        )
        .await
        .unwrap_err();
        assert_eq!(error, erase::EraseError::VaultUnavailable.l10n());
        assert!(accept.1.lock().unwrap().is_empty(), "nothing asked");
        assert_eq!(writers.stopped.load(Ordering::SeqCst), 0);
        assert!(history.exists());
    }

    #[tokio::test]
    async fn a_second_erase_while_one_runs_is_refused() {
        let _serial = SERIAL.lock().await;
        let (_dir, roots, history, _) = tree();
        let writers = writers(&history);
        let held = RUNNING.try_lock().unwrap();
        let error = run(
            "ERASE",
            false,
            roots,
            "en-US",
            &Answer(true, Mutex::new(Vec::new())),
            &writers,
            &vault(true),
            &none,
        )
        .await
        .unwrap_err();
        drop(held);
        assert_eq!(error, execution_review::pending());
        assert!(history.exists());
    }

    #[tokio::test]
    async fn the_plan_lists_the_accounts_file_as_kept() {
        let (_dir, roots, _, _) = tree();
        let view = plan_view(roots, "en-US");
        assert_eq!(view.word, "ERASE");
        let accounts = view
            .items
            .iter()
            .find(|i| i.item.id == "config.accounts")
            .unwrap();
        assert!(!accounts.item.erase);
        assert!(accounts.label.is_none());
        assert!(view.items.iter().any(|i| i.item.erase));
    }

    #[tokio::test]
    async fn vault_items_carry_a_localized_label_beside_the_service_name() {
        let (_dir, roots, _, _) = tree();
        let view = plan_view(roots, "pt-BR");
        let oauth = view
            .items
            .iter()
            .find(|i| i.item.id == "vault.oauth")
            .unwrap();
        assert_eq!(oauth.item.location, "cuotai.oauth");
        assert_eq!(
            oauth.label.as_deref(),
            Some("Logins, chaves e endereços de canais salvos")
        );
        let json = serde_json::to_value(oauth).unwrap();
        assert_eq!(json["location"], "cuotai.oauth");
        assert_eq!(json["label"], "Logins, chaves e endereços de canais salvos");
    }

    /// PRD 0051 re-validation S-01: the closing message names what was left
    /// in place, and only when something was.
    #[test]
    fn the_closing_message_adds_the_left_in_place_note() {
        let done = erase::done().resolve("en-US");
        assert_eq!(closing_message("en-US", 0), done);
        let with = closing_message("en-US", 2);
        assert!(with.starts_with(&done) && with.contains('2'), "{with}");
    }

    /// PRD 0052 re-validation S-02: data left in the default folder before
    /// a move is listed (and so erased) beside the data folder in use.
    #[test]
    fn the_plan_lists_data_left_in_the_default_folder() {
        let (dir, roots, _history, _accounts) = tree();
        let default = dir.path().join("os-default");
        std::fs::create_dir_all(default.join("logs")).unwrap();
        std::fs::write(default.join("logs").join("cuotai.log"), b"x").unwrap();
        cuotai_core::data_dir::mark_for_tests(&default).unwrap();
        let roots = roots.with_erase_roots([default.clone()]);
        let view = plan_view(roots, "en-US");
        let log = cuotai_core::diagnostics::redact::home_abbreviated(
            &default.join("logs").join("cuotai.log"),
        );
        assert!(
            view.items
                .iter()
                .any(|i| i.item.id == "app.log" && i.item.location == log && i.item.erase),
            "{log}"
        );
    }

    /// PRD 0052 S-04: a running `cuotai --serve` stops the erase after the
    /// word and before the vault, the native dialog or the writers.
    #[tokio::test]
    async fn a_running_serve_stops_the_erase_before_anything_is_asked_or_stopped() {
        let _serial = SERIAL.lock().await;
        let (_dir, roots, history, _) = tree();
        let confirmer = Answer(true, Mutex::new(Vec::new()));
        let writers = writers(&history);
        let vault = vault(true);
        let serve = || vec![Program::Serve, Program::Dashboard];
        let error = run(
            "ERASE", false, roots, "en-US", &confirmer, &writers, &vault, &serve,
        )
        .await
        .unwrap_err();
        assert_eq!(error.id, "privacy.erase.running");
        let text = error.resolve("en-US");
        assert!(text.contains("cuotai --serve"), "{text}");
        assert!(text.contains("terminal dashboard"), "{text}");
        assert!(history.exists());
        assert_eq!(writers.stopped.load(Ordering::SeqCst), 0);
        assert!(confirmer.1.lock().unwrap().is_empty(), "nothing asked");
        assert_eq!(vault.searched.load(Ordering::SeqCst), 0);
    }
}
