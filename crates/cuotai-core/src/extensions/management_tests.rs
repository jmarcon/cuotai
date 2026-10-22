//! PRD 0017 RF5: the UI-neutral manager. Discovery rows (including folders
//! whose manifest did not read and accounts of them), the single-use
//! disclosure candidate, the account it builds, one extension per account,
//! reapproval clearing the previous failure, revocation racing a queued run,
//! and the full lifecycle through the real [`crate::Monitor`] in the three
//! shipped locales.

use super::*;
use crate::extensions::MANIFEST_FILE;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Arc as StdArc;
use std::time::Duration;

/// A ready output of one metric.
const READY: &str = concat!(
    r#"{"schema":1,"status":"ready","metrics":[{"label":"Monthly","used":1,"limit":2,"#,
    r#""remaining":1,"unit":"requests","kind":"quota","window":"monthly"}],"notes":[]}"#
);

/// An extension folder with a manifest for `id` and `program`.
///
/// `program` is compiled at runtime by `rustc` when the platform needs a
/// native executable (`program.exe`), and is a shell script otherwise: the
/// manifest always names one file inside the folder, as production does.
struct Rig {
    _dir: tempfile::TempDir,
    root: PathBuf,
    manager: Arc<Manager>,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("extensions");
        fs::create_dir_all(&root).unwrap();
        Self {
            manager: Arc::new(Manager::at(&root)),
            root,
            _dir: dir,
        }
    }

    /// The file `command[0]` must name on this platform.
    fn program_name() -> &'static str {
        if cfg!(windows) {
            "program.exe"
        } else {
            "./run.sh"
        }
    }

    /// An extension whose program prints `body`.
    fn extension(&self, id: &str, body: &str) {
        let dir = self.root.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(MANIFEST_FILE),
            format!(
                "id = \"{id}\"\nname = \"{id}\"\nversion = \"1\"\nschema = 1\n\
                 command = [\"{}\"]\ninterval_seconds = 60\n",
                Self::program_name()
            ),
        )
        .unwrap();
        self.write_program(&dir, body);
    }

    /// The native executable: a real binary built here with `rustc`, so the
    /// same fixture runs on every platform without a shell.
    fn write_program(&self, dir: &Path, body: &str) {
        let source = format!(
            "fn main() {{ use std::io::Write; std::io::stdout().write_all({body:?}.as_bytes()).unwrap(); }}\n"
        );
        let source_file = dir.join("program.rs");
        fs::write(&source_file, source).unwrap();
        self.compile_program(dir, &source_file);
    }

    /// Build `source_file` into the extension's `command[0]`.
    fn compile_program(&self, dir: &Path, source_file: &Path) {
        let program = dir.join(Self::program_name().trim_start_matches("./"));
        let output = std::process::Command::new("rustc")
            .args(["--edition", "2021", "-O"])
            .arg(source_file)
            .arg("-o")
            .arg(&program)
            .output()
            .expect("rustc builds the fixture program");
        assert!(
            output.status.success(),
            "fixture build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// An extension whose program is compiled from `source`.
    fn extension_with_source(&self, id: &str, source: &str) {
        let dir = self.root.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(MANIFEST_FILE),
            format!(
                "id = \"{id}\"\nname = \"{id}\"\nversion = \"1\"\nschema = 1\n\
                 command = [\"{}\"]\ninterval_seconds = 60\n",
                Self::program_name()
            ),
        )
        .unwrap();
        let source_file = dir.join("program.rs");
        fs::write(&source_file, source).unwrap();
        self.compile_program(&dir, &source_file);
    }

    fn rewrite(&self, id: &str, body: &str) {
        self.write_program(&self.root.join(id), body);
    }
}

/// `Config::default()` ships one account per built-in provider; the tests
/// here are about the extension, so the base is explicitly empty.
fn base() -> Config {
    Config {
        accounts: Vec::new(),
        ..Config::default()
    }
}

fn item<'a>(items: &'a [ExtensionItem], id: &str) -> &'a ExtensionItem {
    items
        .iter()
        .find(|item| item.id == id)
        .unwrap_or_else(|| panic!("{id} listed: {:?}", ids(items)))
}

fn ids(items: &[ExtensionItem]) -> Vec<&str> {
    items.iter().map(|item| item.id.as_str()).collect()
}

/// RF5.4: a folder whose manifest does not read is listed with its reason
/// and can never be enabled, while valid extensions still work.
#[test]
fn an_unreadable_manifest_is_listed_and_never_enableable() {
    let rig = Rig::new();
    rig.extension("good", "");
    fs::create_dir_all(rig.root.join("broken")).unwrap();
    fs::write(
        rig.root.join("broken").join(MANIFEST_FILE),
        "id = \"broken\"\nschema = 1\n",
    )
    .unwrap();

    let items = rig.manager.list(&base());
    let broken = item(&items, "broken");
    assert_eq!(broken.state, ExtensionState::Invalid);
    assert!(!broken.state.can_enable());
    assert!(
        broken.error.is_some(),
        "the reason is shown, not a silent drop"
    );
    assert_eq!(item(&items, "good").state, ExtensionState::Disabled);
    assert!(
        rig.manager.disclosure("broken").is_err(),
        "nothing to approve"
    );
}

/// RF5.4: a configured account whose manifest stopped parsing is merged
/// into its own row (Invalid with the reason), never duplicated, never
/// reported as missing.
#[test]
fn a_configured_account_of_a_broken_folder_is_one_invalid_row() {
    let rig = Rig::new();
    let disclosure = rig.manager.disclosure("proxy").unwrap_or_else(|_| {
        rig.extension("proxy", "");
        rig.manager.disclosure("proxy").unwrap()
    });
    let enabled = rig
        .manager
        .enable(&disclosure.candidate, true, &base())
        .unwrap();
    let config = enabled.config;
    // The manifest stops parsing; discovery now rejects the folder.
    fs::write(
        rig.root.join("proxy").join(MANIFEST_FILE),
        "id = \"proxy\"\nschema = 1\n",
    )
    .unwrap();

    let items = rig.manager.list(&config);
    assert_eq!(
        items.iter().filter(|item| item.id == "proxy").count(),
        1,
        "no duplicate row: {:?}",
        ids(&items)
    );
    let row = item(&items, "proxy");
    assert_eq!(row.state, ExtensionState::Invalid);
    assert!(row.account.is_some(), "its account is still listed");
    assert!(row.error.is_some(), "with the rejection reason");
    assert!(row.approved, "and its approval is still revocable");
}

/// RF5.3: the candidate is single use and the approval binds to the
/// reviewed bytes.
#[test]
fn a_candidate_is_single_use_and_binds_to_the_reviewed_files() {
    let rig = Rig::new();
    rig.extension("proxy", "");
    let mut config = base();

    let disclosure = rig.manager.disclosure("proxy").unwrap();
    assert!(!disclosure.lines.is_empty(), "nothing undisclosed");
    config = rig
        .manager
        .enable(&disclosure.candidate, true, &config)
        .unwrap()
        .config;
    assert!(rig.manager.is_enabled("proxy").unwrap());
    assert_eq!(
        item(&rig.manager.list(&config), "proxy").state,
        ExtensionState::Enabled
    );

    // Files changed between the disclosure and the confirmation: refused.
    rig.rewrite("proxy", "changed");
    let fresh = rig.manager.disclosure("proxy").unwrap();
    let replay = rig
        .manager
        .enable(&disclosure.candidate, true, &config)
        .unwrap_err();
    assert_eq!(replay.id, "extensions.action.candidate_expired");
    assert_eq!(fresh.candidate.token.len(), 36, "an opaque UUID token");

    // A second confirmation of the same token is refused as well.
    config = rig
        .manager
        .enable(&fresh.candidate, true, &config)
        .unwrap()
        .config;
    let spent = rig
        .manager
        .enable(&fresh.candidate, true, &config)
        .unwrap_err();
    assert_eq!(spent.id, "extensions.action.candidate_expired");
}

/// RF5.10: reapproval clears the cached failure and the backoff, so the new
/// version runs at once. The first program runs and fails (an exit code
/// other than zero), never a file change, so the approval stays valid.
#[tokio::test]
async fn reapproval_runs_the_new_version_at_once() {
    let rig = Rig::new();
    // The failing program exists BEFORE the disclosure, so the approval is
    // made of a program that starts and exits non-zero (a valid approval of
    // a program that simply fails, not a file change).
    let source = rig.root.join("proxy").join("program.rs");
    rig.extension_with_source(
        "proxy",
        r#"fn main() { use std::io::Write; std::io::stdout().write_all(b"{\"schema\":1,\"status\":\"error\"}").unwrap(); std::process::exit(3); }"#,
    );
    let mut config = base();

    let disclosure = rig.manager.disclosure("proxy").unwrap();
    let enabled = rig
        .manager
        .enable(&disclosure.candidate, true, &config)
        .unwrap();
    config = enabled.config;

    let failed = rig.manager.test("proxy").await.unwrap();
    assert_eq!(failed.status, Status::Error, "the run failed");
    let listed = rig.manager.list(&config);
    let row = item(&listed, "proxy");
    assert!(row.error.is_some(), "the failure is listed");

    // The same program now prints a ready reading; re-disclose, reapprove.
    fs::write(
        &source,
        format!(
            "fn main() {{ use std::io::Write; std::io::stdout().write_all({READY:?}.as_bytes()).unwrap(); }}"
        ),
    )
    .unwrap();
    rig.compile_program(&rig.root.join("proxy"), &source);
    let listed = rig.manager.list(&config);
    let row = item(&listed, "proxy");
    assert!(row.error.is_some(), "the failure is listed");

    // The same program now prints a ready reading; re-disclose, reapprove.
    let fresh = rig.manager.disclosure("proxy").unwrap();
    config = rig
        .manager
        .enable(&fresh.candidate, true, &config)
        .unwrap()
        .config;
    // The frontend's configuration write succeeded: core clears the old
    // version's cached failure and backoff.
    rig.manager.confirm_commit("proxy");
    let listed = rig.manager.list(&config);
    let row = item(&listed, "proxy");
    assert!(row.error.is_none(), "reapproval cleared the failure");
    let now = rig.manager.test("proxy").await.unwrap();
    assert_eq!(now.status, Status::Ready, "the new version ran at once");
}

/// RF5.2: one extension is one account; two rows for the same id are a
/// configuration error whatever their names.
#[test]
fn one_extension_is_at_most_one_account() {
    let account = |name: &str, id: &str| Account {
        name: name.to_owned(),
        provider: Provider::Extension,
        params: BTreeMap::from([(EXTENSION_ID_PARAM.to_owned(), id.to_owned())]),
        refresh_seconds: Some(300),
        ..Account::default()
    };
    let one = Config {
        accounts: vec![account("Proxy", "proxy")],
        ..Config::default()
    };
    assert!(one.validate().is_ok());
    let twice = Config {
        accounts: vec![account("Proxy", "proxy"), account("Proxy copy", "proxy")],
        ..Config::default()
    };
    assert!(matches!(
        twice.validate(),
        Err(crate::ConfigError::Extension)
    ));
    let two_extensions = Config {
        accounts: vec![account("Proxy", "proxy"), account("Other", "other")],
        ..Config::default()
    };
    assert!(two_extensions.validate().is_ok());
}

/// RF5.5: revocation wins over a run still queued behind the two-process
/// cap. Both slots are held by programs that announce themselves through
/// marker files, so the test waits for real occupancy; the third run is
/// spawned only afterwards and the runner reports when it is waiting for a
/// permit, so the revocation provably lands while it is still queued. The
/// holders are released through a file the test creates, never a sleep.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_revocation_beats_a_queued_run() {
    let rig = StdArc::new(Rig::new());
    let holders = ["busy_a", "busy_b"];
    // Each holder writes its marker into its own working directory (the
    // extension folder), which is where the process actually runs.
    for id in holders {
        rig.extension_with_source(id, &hold_source());
    }
    let late_marker = rig.root.join("late").join("started");
    rig.extension_with_source("late", &marking_source(&late_marker, READY));
    for id in holders.iter().chain(std::iter::once(&"late")) {
        let disclosure = rig.manager.disclosure(id).unwrap();
        rig.manager
            .enable(&disclosure.candidate, true, &base())
            .unwrap();
    }
    let account = |id: &str| Account {
        name: id.to_owned(),
        provider: Provider::Extension,
        params: BTreeMap::from([(EXTENSION_ID_PARAM.to_owned(), id.to_owned())]),
        refresh_seconds: Some(60),
        ..Account::default()
    };

    let first = StdArc::clone(&rig);
    let holder = account(holders[0]);
    let running = tokio::spawn(async move { first.manager.run_account(&holder).await });
    let second = StdArc::clone(&rig);
    let holder = account(holders[1]);
    let other = tokio::spawn(async move { second.manager.run_account(&holder).await });

    // Both slots are really occupied (each wrote its marker in its own
    // folder) before the third run is even started, so it cannot have run.
    for id in holders {
        await_file(&rig.root.join(id).join("started"), "holder started").await;
    }
    let queued_manager = StdArc::clone(&rig);
    let late = account("late");
    let waiting = queued_manager.manager.waiting();
    let queued = tokio::spawn(async move {
        let result = queued_manager.manager.run_account(&late).await;
        (result, queued_manager.manager.waiting())
    });
    // It has passed consent and is now waiting for a permit; only then does
    // the revocation land. Both waits fail the test when unmet.
    await_count(&waiting, 1, "the late run queued for a permit").await;
    rig.manager.disable("late").unwrap();

    // Free both slots immediately after revocation. The queued task can only
    // observe revocation once it acquires a permit, so awaiting it first would
    // deadlock until the holders' manifest timeout.
    for id in holders {
        fs::write(rig.root.join(id).join("release"), b"go").unwrap();
    }

    let (refused, _) = queued.await.unwrap();
    let refused = refused.unwrap();
    assert_eq!(refused.status, Status::Error, "the queued run did not run");
    assert!(
        refused
            .notes
            .iter()
            .any(|note| note.id.as_ref() == super::super::runner::REVOKED_NOTE),
        "revoked: {:?}",
        refused.notes
    );
    assert!(
        !late_marker.exists(),
        "the program must never have started after the revocation"
    );

    running.await.unwrap().unwrap();
    other.await.unwrap().unwrap();
}

/// A program that announces itself in its working directory and then waits
/// for a `release` file there to appear, bounded by the manifest's own
/// timeout.
fn hold_source() -> String {
    "fn main() {\n    std::fs::write(\"started\", b\"1\").ok();\n    for _ in 0..500 {\n        if std::path::Path::new(\"release\").exists() { return; }\n        std::thread::sleep(std::time::Duration::from_millis(20));\n    }\n}\n"
        .to_owned()
}

/// A program that announces itself and prints `out`.
fn marking_source(marker: &Path, out: &str) -> String {
    format!(
        "fn main() {{\n    if let Some(dir) = std::path::Path::new({marker:?}).parent() {{ std::fs::write(dir.join(\"started\"), b\"1\").ok(); }}\n    print!(\"{{}}\", {out:?});\n}}\n"
    )
}

/// Wait until `path` exists; failing the test (not silently continuing)
/// when it never does.
async fn await_file(path: &Path, what: &str) {
    for _ in 0..1000 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{what} never happened: {}", path.display());
}

/// Wait until at least `count` runs are queued for a permit, failing the
/// test when that never happens.
async fn await_count(waiting: &AtomicUsize, count: usize, what: &str) {
    for _ in 0..1000 {
        if waiting.load(Ordering::SeqCst) >= count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{what} ({} waiting)", waiting.load(Ordering::SeqCst));
}

/// RF5.12: a slow extension at index 0 must not delay a built-in account
/// when `concurrency` is 1. The extension runs under the runner's own cap,
/// outside the provider semaphore, and the merged snapshot keeps the
/// configuration order.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_slow_extension_does_not_delay_a_builtin_account() {
    let rig = StdArc::new(Rig::new());
    rig.extension_with_source(
        "slow",
        &format!(
            "fn main() {{\n    std::fs::write(\"started\", b\"1\").ok();\n    for _ in 0..200 {{\n        if std::path::Path::new(\"release\").exists() {{ print!(\"{{}}\", {READY:?}); return; }}\n        std::thread::sleep(std::time::Duration::from_millis(10));\n    }}\n}}\n"
        ),
    );
    let disclosure = rig.manager.disclosure("slow").unwrap();
    let enabled = rig
        .manager
        .enable(&disclosure.candidate, true, &base())
        .unwrap();
    rig.manager.confirm_commit("slow");
    let mut config = enabled.config;
    // The extension is first, so it would block a serial fan-out.
    assert_eq!(config.accounts[0].provider, Provider::Extension);
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })),
        )
        .mount(&server)
        .await;
    struct Credentials;
    impl crate::Credentials for Credentials {
        fn get(&self, _name: &str) -> Option<String> {
            Some("gsk_test".to_owned())
        }
    }
    config.accounts.push(Account {
        name: "builtin".into(),
        provider: Provider::Groq,
        credential_env: Some("GROQ_TEST_KEY".into()),
        ..Account::default()
    });
    config.concurrency = 1;
    config.validate().unwrap();

    let mut monitor = crate::Monitor::with_credentials(config.clone(), Arc::new(Credentials))
        .unwrap()
        .with_extensions(Arc::clone(&rig.manager));
    monitor.groq_url = Some(server.uri());
    let slow_root = rig.root.join("slow");
    let refresh = tokio::spawn(async move { monitor.refresh().await });
    await_file(&slow_root.join("started"), "the slow extension started").await;
    // The extension is still blocked. Seeing the provider request now proves
    // the built-in fan-out was not queued behind it at concurrency 1.
    for _ in 0..1000 {
        if !server.received_requests().await.unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        !server.received_requests().await.unwrap().is_empty(),
        "the built-in query did not complete while the extension was held"
    );
    fs::write(slow_root.join("release"), b"go").unwrap();
    let snapshot = refresh.await.unwrap();
    // The snapshot is in configuration order and holds both reports.
    assert_eq!(snapshot.reports.len(), 2);
    assert_eq!(snapshot.reports[0].provider, Provider::Extension);
    assert_eq!(snapshot.reports[0].status, Status::Ready);
    assert_eq!(snapshot.reports[1].provider, Provider::Groq);
}

/// RF5 end to end, in each shipped locale: enable through the disclosure,
/// a normal Monitor refresh yielding an `Extension` report with the access
/// label, then disable (which stops the next run) and the ordinary account
/// removal (which leaves the installed folder untouched).
#[tokio::test]
async fn enable_refresh_disable_and_remove_in_every_locale() {
    for locale in ["pt-BR", "en-US", "es", "it"] {
        let rig = Rig::new();
        rig.extension("quota", READY);

        // Enable: the disclosure names what will run, in this locale.
        let disclosure = rig.manager.disclosure("quota").unwrap();
        let shown: Vec<String> = disclosure
            .lines
            .iter()
            .map(|line| line.resolve(locale))
            .collect();
        let joined = shown.join("\n");
        assert!(joined.contains("quota"), "{locale}: {joined}");
        assert!(
            joined.contains("60 s"),
            "{locale}: interval and timeout disclosed: {joined}"
        );

        // The real monitor runs it like any configured account.
        let enabled = rig
            .manager
            .enable(&disclosure.candidate, true, &base())
            .unwrap();
        let config = enabled.config;
        let index = enabled
            .action
            .account
            .as_ref()
            .expect("the enabled account")
            .index;
        assert_eq!(config.accounts.len(), 1);
        let monitor = crate::Monitor::new(config.clone())
            .unwrap()
            .with_extensions(Arc::clone(&rig.manager));
        let report = monitor.refresh_account(index).await;
        assert_eq!(report.provider, Provider::Extension);
        assert_eq!(
            report.status,
            Status::Ready,
            "{locale}: expected ready status"
        );
        assert_eq!(report.source, "extension:quota");
        let access = crate::access::describe(&config.accounts[index]);
        assert_eq!(access.level, crate::access::Level::Extension);
        assert_eq!(
            access.level.label().resolve(locale),
            match locale {
                "pt-BR" => "Extensão",
                "en-US" => "Extension",
                "it" => "Estensione",
                _ => "Extensión",
            },
            "{locale}: the card's access label"
        );

        // Disable: the approval is revoked and the next run refuses to
        // start, isolated in its own error reading.
        rig.manager.disable("quota").unwrap();
        let after = monitor.refresh_account(index).await;
        assert_eq!(after.status, Status::NeedsConfiguration, "{locale}");
        assert!(
            after
                .notes_l10n
                .iter()
                .any(|note| note.id.as_ref() == "extensions.consent.not_approved"),
            "{locale}: the refusal is explained, not silent"
        );

        // Removal is the ordinary account removal: the folder stays.
        let without = config.without_account(index).unwrap();
        assert!(without.accounts.is_empty());
        assert!(rig.root.join("quota").join(MANIFEST_FILE).exists());
        assert_eq!(
            rig.manager.list(&without).first().map(|item| item.state),
            Some(ExtensionState::Disabled),
            "{locale}: approved nothing, so it is disabled again"
        );
    }
}
