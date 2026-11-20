//! PRD 0052 RF5.1/RF5.3 — the command-line data actions: history export
//! and erase (PRD 0006 RF4.4 parity) and erase all Cuotai data. Every
//! storage rule lives in the core (`HistoryStore`, `privacy::erase`); this
//! module only shows what will happen, asks, and reports. Exit codes:
//! 0 done, 2 partly failed, 1 refused or invalid.

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use cuotai_core::config_file::ConfigFile;
use cuotai_core::history::{ExportFormat, HistoryStore, Retention};
use cuotai_core::oauth::{EraseVault, StoreChoice};
use cuotai_core::output_file::Protected;
use cuotai_core::privacy::erase::{self, EraseOptions, Outcome, Program, SystemVault};
use cuotai_core::privacy::Roots;

use crate::cli::{self, Field, Level};
use crate::l10n::{tr, trf};

type Res = Result<u8, Box<dyn std::error::Error>>;

/// The core's confirmation word (shared with the desktop and the TUI
/// history erase), localized; compared exactly.
pub(crate) fn word() -> String {
    erase::confirmation_word().resolve(crate::ui::tui_locale())
}

/// Ask for the exact word on a terminal; without one, refuse unless `yes`.
/// `Ok(false)` when the person typed something else.
fn confirmed(yes: bool, input: &mut dyn BufRead, interactive: bool) -> Result<bool, String> {
    if yes {
        return Ok(true);
    }
    if !interactive {
        return Err(tr(
            "cli.erase_needs_yes",
            "No terminal to confirm on: nothing was erased. Run again with --yes to confirm.",
        )
        .to_string());
    }
    eprint!(
        "{}",
        trf(
            "cli.erase_prompt",
            "Type {word} to confirm: ",
            &[("word", &word())]
        )
    );
    let _ = io::stderr().flush();
    let mut typed = String::new();
    input
        .read_line(&mut typed)
        .map_err(|_| tr("cli.erase_cancelled", "Nothing was erased.").to_string())?;
    Ok(typed.trim() == word().as_str())
}

fn terminal() -> bool {
    io::stdin().is_terminal() && io::stderr().is_terminal()
}

/// `--history-export PATH --history-format F [--history ACCOUNT]
/// [--history-days N]`.
pub(crate) fn history_export(
    config: Option<PathBuf>,
    out: &Path,
    format: ExportFormat,
    account: Option<&str>,
    days: Option<u32>,
) -> Res {
    let locale = crate::ui::tui_locale();
    // `--history-days` means 7 to 365 days wherever it appears (`--history`,
    // the retention); checked before anything is read.
    if let Some(days) = days {
        Retention::days(days).map_err(|e| e.l10n().resolve(locale))?;
    }
    let start =
        HistoryStore::export_start(days, Utc::now()).map_err(|e| e.l10n().resolve(locale))?;
    let file = crate::load_config(config, false)?;
    let store = HistoryStore::in_data_dir(&file).map_err(|e| e.l10n().resolve(locale))?;
    let accounts = select(&file, account)?;
    let protected = Protected::system(Some(&file.path));
    let rows = store
        .export_to_file(&accounts, start, format, out, &protected)
        .map_err(|e| e.l10n().resolve(locale))?;
    cli::message_with(
        Level::Info,
        "history.exported",
        &[("rows", Field::Count(rows as u64))],
        exported_text(rows, &out.display()),
    );
    Ok(0)
}

/// "N history rows written to PATH." (CLI and TUI).
pub(crate) fn exported_text(rows: usize, path: &dyn std::fmt::Display) -> String {
    trf(
        "cli.history_exported",
        "{rows} history rows written to {path}.",
        &[("rows", &rows), ("path", path)],
    )
}

/// What a history erase will remove: one account (`name`, already made
/// safe for display) or every account (CLI and TUI).
pub(crate) fn erase_notice(name: Option<&str>) -> String {
    match name {
        None => tr(
            "cli.history_erase_all",
            "The history of every account on this device will be erased.",
        )
        .to_string(),
        Some(name) => trf(
            "cli.history_erase_account",
            "The history of {name} will be erased.",
            &[("name", &name)],
        ),
    }
}

/// "N history samples erased." (CLI and TUI).
pub(crate) fn erased_text(count: usize) -> String {
    trf(
        "cli.history_erased",
        "{count} history samples erased.",
        &[("count", &count)],
    )
}

fn select(file: &ConfigFile, name: Option<&str>) -> Result<Vec<cuotai_core::Account>, String> {
    let accounts: Vec<_> = file
        .config
        .accounts
        .iter()
        .filter(|a| name.is_none_or(|n| a.name == n || a.display_name() == n))
        .cloned()
        .collect();
    match name {
        Some(name) if accounts.is_empty() => Err(trf(
            "cli.account_not_found",
            "Account not found in the configuration: {name}",
            &[("name", &crate::ui::safe(name))],
        )),
        _ => Ok(accounts),
    }
}

/// `--history-erase ACCOUNT|all [--yes]`.
pub(crate) fn history_erase(config: Option<PathBuf>, target: &str, yes: bool) -> Res {
    let file = crate::load_config(config, false)?;
    let locale = crate::ui::tui_locale();
    let store = HistoryStore::in_data_dir(&file).map_err(|e| e.l10n().resolve(locale))?;
    let all = target == "all";
    let accounts = if all {
        Vec::new()
    } else {
        select(&file, Some(target))?
    };
    let safe = crate::ui::safe(target);
    eprintln!("{}", erase_notice((!all).then_some(&*safe)));
    match confirmed(yes, &mut io::stdin().lock(), terminal()) {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("{}", tr("cli.erase_cancelled", "Nothing was erased."));
            return Ok(1);
        }
        Err(text) => {
            eprintln!("{text}");
            return Ok(1);
        }
    }
    let removed = if all {
        store.erase_all()
    } else {
        store.erase_account(&accounts[0])
    }
    .map_err(|e| e.l10n().resolve(locale))?;
    cli::message_with(
        Level::Info,
        "history.erased",
        &[("samples", Field::Count(removed as u64))],
        erased_text(removed),
    );
    Ok(0)
}

/// The accounts TOML in use, when one can be found (erase never needs it
/// to load).
fn accounts_path(explicit: Option<PathBuf>) -> Option<PathBuf> {
    ConfigFile::load(explicit, false)
        .ok()
        .filter(|file| file.exists)
        .map(|file| file.path)
}

/// `--erase-all-data [--yes] [--include-config]`, on the credential store
/// this process was started with (PRD 0050 `SEC-DEV-VAULT`): with
/// `--dev-credentials-sqlite` it clears that store and never the system
/// vault; a release build refuses the flag before anything is listed. It
/// also erases the desktop app's chosen data folder and data left in the OS
/// default one ([`Roots::for_erase`], PRD 0052 re-validation S-02).
/// PRD 0052 S-04: it refuses while another Cuotai program runs
/// ([`cuotai_core::presence::running`]).
pub(crate) fn erase_all_data(
    config: Option<PathBuf>,
    include_config: bool,
    yes: bool,
    choice: &StoreChoice,
) -> Res {
    let options = EraseOptions {
        roots: Roots::for_erase(accounts_path(config)),
        include_config,
    };
    erase_all_data_with(
        &options,
        yes,
        choice,
        || Box::new(SystemVault),
        &cuotai_core::presence::running,
        &mut io::stdin().lock(),
        terminal(),
        &mut io::stdout().lock(),
    )
}

/// [`erase_all_data`] over any roots, input, system-vault factory and
/// running-program check (tests): the vault comes from `choice`, which
/// calls `system` only for the system store. A store that cannot open is a
/// local error (exit 1) and nothing is listed or erased.
#[allow(clippy::too_many_arguments)]
pub(crate) fn erase_all_data_with(
    options: &EraseOptions,
    yes: bool,
    choice: &StoreChoice,
    system: impl FnOnce() -> EraseVault,
    running: &dyn Fn() -> Vec<Program>,
    input: &mut dyn BufRead,
    interactive: bool,
    out: &mut dyn Write,
) -> Res {
    let vault = choice
        .erase_vault_with(system)
        .map_err(|text| text.resolve(crate::ui::tui_locale()))?;
    erase_with(options, &*vault, running, yes, input, interactive, out)
}

/// PRD 0052 S-04: `true` (after printing what to quit on stderr) when
/// another Cuotai program runs and the erase must stop.
fn refused_while_running(running: &dyn Fn() -> Vec<Program>) -> bool {
    let programs = running();
    if programs.is_empty() {
        return false;
    }
    let locale = crate::ui::tui_locale();
    eprintln!(
        "cuotai: {}",
        erase::running(&programs, locale).resolve(locale)
    );
    true
}

/// The flow of [`erase_all_data`] over any vault, running-program check
/// and input (tests).
pub(crate) fn erase_with(
    options: &EraseOptions,
    vault: &dyn erase::Vault,
    running: &dyn Fn() -> Vec<Program>,
    yes: bool,
    input: &mut dyn BufRead,
    interactive: bool,
    out: &mut dyn Write,
) -> Res {
    // PRD 0052 S-04: a running app, server, watcher or dashboard could
    // write its files back; nothing is listed, asked or erased.
    if refused_while_running(running) {
        return Ok(1);
    }
    let plan = erase::plan(options);
    writeln!(out, "{}", tr("cli.erase_plan_header", "Cuotai will erase:"))?;
    for item in plan.iter().filter(|i| i.erase) {
        writeln!(out, "  {:<22} {}", item.id, item.location)?;
    }
    let kept: Vec<_> = plan
        .iter()
        .filter(|i| !i.erase && i.reason.is_none())
        .collect();
    if !kept.is_empty() {
        writeln!(
            out,
            "{}",
            tr(
                "cli.erase_kept_header",
                "Kept (add --include-config to erase them too):"
            )
        )?;
        for item in kept {
            writeln!(out, "  {:<22} {}", item.id, item.location)?;
        }
    }
    // Data folders Cuotai did not create: kept whatever the flags say.
    for item in plan.iter().filter(|i| !i.erase) {
        if let Some(reason) = &item.reason {
            writeln!(
                out,
                "  {}  {}",
                item.location,
                reason.resolve(crate::ui::tui_locale())
            )?;
        }
    }
    out.flush()?;
    match confirmed(yes, input, interactive) {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("{}", tr("cli.erase_cancelled", "Nothing was erased."));
            return Ok(1);
        }
        Err(text) => {
            eprintln!("{text}");
            return Ok(1);
        }
    }
    // Checked again after the confirmation, which can take a while: a
    // program started meanwhile stops the erase too.
    if refused_while_running(running) {
        return Ok(1);
    }
    // The app log is erased too: nothing is logged after this point.
    let report = match erase::erase_all(options, vault) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("cuotai: {}", error.l10n().resolve(crate::ui::tui_locale()));
            return Ok(1);
        }
    };
    // PRD 0051 re-validation S-01: what Cuotai did not create stays.
    let foreign = report.count(Outcome::Foreign);
    if report.complete() {
        eprintln!(
            "{}",
            trf(
                "cli.erase_done",
                "Erased: {removed} removed, {absent} already absent.",
                &[
                    ("removed", &report.count(Outcome::Removed)),
                    ("absent", &report.count(Outcome::Absent)),
                ],
            )
        );
        if foreign > 0 {
            eprintln!(
                "{}",
                erase::foreign(foreign).resolve(crate::ui::tui_locale())
            );
        }
        Ok(0)
    } else {
        eprintln!(
            "{}",
            trf(
                "cli.erase_failed",
                "Could not erase: {ids}. Run the command again to retry.",
                &[("ids", &report.failed().join(", "))],
            )
        );
        Ok(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct Vault {
        searchable: bool,
        deleted: Cell<usize>,
    }

    impl erase::Vault for Vault {
        fn can_search(&self, _: &str) -> bool {
            self.searchable
        }
        fn delete_service(&self, _: &str) -> Result<usize, erase::VaultFailure> {
            self.deleted.set(self.deleted.get() + 1);
            Ok(1)
        }
    }

    fn options(dir: &Path) -> EraseOptions {
        let roots = Roots {
            local_data: Some(dir.join("local").join("cuotai")),
            config: Some(dir.join("config")),
            accounts: Some(dir.join("cuotai.toml")),
            mac_home: None,
            metered: None,
            feed: None,
            ..Roots::default()
        };
        std::fs::create_dir_all(dir.join("local/cuotai/logs")).unwrap();
        cuotai_core::data_dir::mark_for_tests(&dir.join("local/cuotai")).unwrap();
        std::fs::write(dir.join("local/cuotai/logs/cuotai.log"), b"x").unwrap();
        std::fs::write(dir.join("cuotai.toml"), b"").unwrap();
        EraseOptions {
            roots,
            include_config: false,
        }
    }

    /// No other Cuotai program runs.
    fn none() -> Vec<Program> {
        Vec::new()
    }

    fn vault() -> Vault {
        Vault {
            searchable: true,
            deleted: Cell::new(0),
        }
    }

    #[test]
    fn without_a_terminal_and_without_yes_nothing_is_erased() {
        let dir = tempfile::tempdir().unwrap();
        let options = options(dir.path());
        let mut out = Vec::new();
        let vault = vault();
        let code = erase_with(
            &options,
            &vault,
            &none,
            false,
            &mut &b""[..],
            false,
            &mut out,
        )
        .unwrap();
        assert_eq!(code, 1);
        assert_eq!(vault.deleted.get(), 0);
        assert!(dir.path().join("local/cuotai/logs/cuotai.log").exists());
        let shown = String::from_utf8(out).unwrap();
        assert!(shown.contains("app.log") && shown.contains("vault.oauth"));
        assert!(shown.contains("config.accounts"), "the kept TOML is listed");
    }

    #[test]
    fn a_terminal_needs_the_exact_word() {
        let dir = tempfile::tempdir().unwrap();
        let options = options(dir.path());
        let vault = vault();
        let code = erase_with(
            &options,
            &vault,
            &none,
            false,
            &mut format!("{}\n", word().to_lowercase()).as_bytes(),
            true,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(code, 1, "case matters");
        let code = erase_with(
            &options,
            &vault,
            &none,
            false,
            &mut format!("{}\n", word()).as_bytes(),
            true,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(code, 0);
        assert!(!dir.path().join("local/cuotai").exists());
        assert!(dir.path().join("cuotai.toml").exists(), "TOML kept");
    }

    #[test]
    fn yes_erases_and_an_unsearchable_vault_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let options = options(dir.path());
        let refusing = Vault {
            searchable: false,
            deleted: Cell::new(0),
        };
        let code = erase_with(
            &options,
            &refusing,
            &none,
            true,
            &mut &b""[..],
            false,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(code, 1);
        assert!(dir.path().join("local/cuotai/logs/cuotai.log").exists());
        let code = erase_with(
            &options,
            &vault(),
            &none,
            true,
            &mut &b""[..],
            false,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(code, 0);
    }

    /// PRD 0052 re-validation S-02: the plan lists the desktop's chosen
    /// folder and data left in the default one, and a folder Cuotai did not
    /// create as kept with the reason.
    #[test]
    fn the_plan_lists_the_desktop_folder_and_the_default_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut options = options(dir.path());
        let chosen = dir.path().join("chosen");
        let default = dir.path().join("os-default");
        let foreign = dir.path().join("theirs");
        for root in [&chosen, &default] {
            std::fs::create_dir_all(root.join("logs")).unwrap();
            std::fs::write(root.join("logs/cuotai.log"), b"x").unwrap();
            cuotai_core::data_dir::mark_for_tests(root).unwrap();
        }
        std::fs::create_dir_all(&foreign).unwrap();
        options.roots =
            options
                .roots
                .with_erase_roots([chosen.clone(), default.clone(), foreign.clone()]);
        let mut out = Vec::new();
        let code = erase_with(
            &options,
            &vault(),
            &none,
            true,
            &mut &b""[..],
            false,
            &mut out,
        )
        .unwrap();
        assert_eq!(code, 0);
        let shown = String::from_utf8(out).unwrap();
        for root in [&chosen, &default] {
            assert!(
                shown.contains(&root.join("logs").join("cuotai.log").display().to_string()),
                "{shown}"
            );
            assert!(!root.exists(), "{} erased", root.display());
        }
        assert!(shown.contains(&foreign.display().to_string()), "{shown}");
        let reason = erase::not_cuotai_folder().resolve(crate::ui::tui_locale());
        assert!(shown.contains(&reason), "{shown}");
        assert!(foreign.exists());
    }

    /// PRD 0051 re-validation S-01: a data root Cuotai did not create (no
    /// marker) is left as it is, and the erase still succeeds.
    #[test]
    fn a_data_folder_cuotai_did_not_create_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let options = options(dir.path());
        std::fs::remove_file(
            dir.path()
                .join("local/cuotai")
                .join(cuotai_core::data_dir::MARKER),
        )
        .unwrap();
        let code = erase_with(
            &options,
            &vault(),
            &none,
            true,
            &mut &b""[..],
            false,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(code, 0);
        assert!(dir.path().join("local/cuotai/logs/cuotai.log").exists());
    }

    /// PRD 0050 `SEC-DEV-VAULT` (S-04): `--erase-all-data` with
    /// `--dev-credentials-sqlite` clears that store and never builds the
    /// system vault.
    // security-invariant: SEC-DEV-VAULT
    #[cfg(debug_assertions)]
    #[test]
    fn erase_on_the_development_store_never_reaches_the_system_vault() {
        let dir = tempfile::tempdir().unwrap();
        let options = options(dir.path());
        let database = dir.path().join("cuotai-dev.sqlite3");
        let choice = StoreChoice::Development(database.clone());
        let store = choice.session().unwrap().token_store();
        let token = cuotai_core::oauth::OAuthToken {
            access_token: "placeholder".into(),
            refresh_token: None,
            expires_at: None,
            metadata: Default::default(),
        };
        store.save("first", &token).unwrap();
        drop(store);
        let code = erase_all_data_with(
            &options,
            true,
            &choice,
            || panic!("the OS vault was reached"),
            &none,
            &mut &b""[..],
            false,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(code, 0);
        assert!(!dir.path().join("local/cuotai").exists());
        assert!(database.exists(), "the store's file is kept");
        let store = choice.session().unwrap().token_store();
        assert!(store.load("first").unwrap().is_none(), "its rows are gone");
    }

    /// PRD 0052 S-04: another Cuotai program running stops the erase before
    /// the plan: nothing is asked or erased, the vault is never touched.
    #[test]
    fn erase_refuses_while_another_cuotai_runs_and_asks_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let options = options(dir.path());
        let vault = vault();
        let mut out = Vec::new();
        let serve = || vec![Program::Serve];
        let code = erase_with(
            &options,
            &vault,
            &serve,
            false,
            &mut format!("{}\n", word()).as_bytes(),
            true,
            &mut out,
        )
        .unwrap();
        assert_eq!(code, 1);
        assert_eq!(vault.deleted.get(), 0);
        assert!(dir.path().join("local/cuotai/logs/cuotai.log").exists());
        assert!(out.is_empty(), "no plan, no prompt");
        let text = erase::running(&[Program::Serve], "en-US").resolve("en-US");
        assert!(text.contains("cuotai --serve"), "{text}");
    }

    /// PRD 0052 S-04: `--yes` does not bypass the check, and a program that
    /// starts after the plan is caught by the second check.
    #[test]
    fn erase_with_yes_still_refuses_while_the_desktop_runs() {
        let dir = tempfile::tempdir().unwrap();
        let options = options(dir.path());
        let vault = vault();
        let desktop = || vec![Program::Desktop];
        let code = erase_with(
            &options,
            &vault,
            &desktop,
            true,
            &mut &b""[..],
            false,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(code, 1);
        // Nothing runs at the first check; the app starts before the second.
        let calls = Cell::new(0);
        let later = || {
            calls.set(calls.get() + 1);
            if calls.get() > 1 {
                vec![Program::Desktop]
            } else {
                Vec::new()
            }
        };
        let mut out = Vec::new();
        let code = erase_with(
            &options,
            &vault,
            &later,
            true,
            &mut &b""[..],
            false,
            &mut out,
        )
        .unwrap();
        assert_eq!(code, 1);
        assert_eq!(calls.get(), 2);
        assert!(
            !out.is_empty(),
            "the plan was shown before the second check"
        );
        assert_eq!(vault.deleted.get(), 0);
        assert!(dir.path().join("local/cuotai/logs/cuotai.log").exists());
    }
}
