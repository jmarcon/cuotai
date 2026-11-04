//! PRD 0052 RF5.1/RF5.3 — the command-line data actions: history export
//! and erase (PRD 0006 RF4.4 parity) and erase all Cuotai data. Every
//! storage rule lives in the core (`HistoryStore`, `privacy::erase`); this
//! module only shows what will happen, asks, and reports. Exit codes:
//! 0 done, 2 partly failed, 1 refused or invalid.

use std::io::{self, BufRead, IsTerminal, Write};
use std::ops::Bound;
use std::path::{Path, PathBuf};

use chrono::{TimeDelta, Utc};
use cuotai_core::config_file::ConfigFile;
use cuotai_core::history::{ExportFormat, HistoryStore};
use cuotai_core::privacy::erase::{self, EraseOptions, Outcome, SystemVault};
use cuotai_core::privacy::Roots;

use crate::cli::{self, Field, Level};
use crate::l10n::{tr, trf};

type Res = Result<u8, Box<dyn std::error::Error>>;

/// The confirmation word, localized; compared exactly.
fn word() -> &'static str {
    tr("cli.erase_word", "ERASE")
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
    Ok(typed.trim() == word())
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
    let file = ConfigFile::load(config, false)?;
    let locale = crate::ui::tui_locale();
    let store = HistoryStore::in_data_dir(&file).map_err(|e| e.l10n().resolve(locale))?;
    let accounts = select(&file, account)?;
    let start = days.map_or(Bound::Unbounded, |days| {
        Bound::Included(Utc::now() - TimeDelta::days(i64::from(days)))
    });
    let handle = private_file(out).map_err(|_| {
        cuotai_core::history::HistoryError::Export
            .l10n()
            .resolve(locale)
    })?;
    let mut writer = io::BufWriter::new(handle);
    let rows = store
        .export(
            &accounts,
            None,
            (start, Bound::Unbounded),
            format,
            &mut writer,
        )
        .map_err(|e| e.l10n().resolve(locale))?;
    writer.flush()?;
    cli::message_with(
        Level::Info,
        "history.exported",
        &[("rows", Field::Count(rows as u64))],
        trf(
            "cli.history_exported",
            "{rows} history rows written to {path}.",
            &[("rows", &rows), ("path", &out.display())],
        ),
    );
    Ok(0)
}

/// A new file only the user can read; an existing file is replaced.
fn private_file(path: &Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
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
    let file = ConfigFile::load(config, false)?;
    let locale = crate::ui::tui_locale();
    let store = HistoryStore::in_data_dir(&file).map_err(|e| e.l10n().resolve(locale))?;
    let all = target == "all";
    let accounts = if all {
        Vec::new()
    } else {
        select(&file, Some(target))?
    };
    eprintln!(
        "{}",
        if all {
            tr(
                "cli.history_erase_all",
                "The history of every account on this device will be erased.",
            )
            .to_string()
        } else {
            trf(
                "cli.history_erase_account",
                "The history of {name} will be erased.",
                &[("name", &crate::ui::safe(target))],
            )
        }
    );
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
        trf(
            "cli.history_erased",
            "{count} history samples erased.",
            &[("count", &removed)],
        ),
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

/// `--erase-all-data [--yes] [--include-config]`.
pub(crate) fn erase_all_data(config: Option<PathBuf>, include_config: bool, yes: bool) -> Res {
    let options = EraseOptions {
        roots: Roots::system(accounts_path(config)),
        include_config,
    };
    erase_with(
        &options,
        &SystemVault,
        yes,
        &mut io::stdin().lock(),
        terminal(),
        &mut io::stdout().lock(),
    )
}

/// The flow of [`erase_all_data`] over any vault and input (tests).
pub(crate) fn erase_with(
    options: &EraseOptions,
    vault: &dyn erase::Vault,
    yes: bool,
    input: &mut dyn BufRead,
    interactive: bool,
    out: &mut dyn Write,
) -> Res {
    let plan = erase::plan(options);
    writeln!(out, "{}", tr("cli.erase_plan_header", "Cuotai will erase:"))?;
    for item in plan.iter().filter(|i| i.erase) {
        writeln!(out, "  {:<22} {}", item.id, item.location)?;
    }
    let kept: Vec<_> = plan.iter().filter(|i| !i.erase).collect();
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
    // The app log is erased too: nothing is logged after this point.
    let report = match erase::erase_all(options, vault) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("cuotai: {}", error.l10n().resolve(crate::ui::tui_locale()));
            return Ok(1);
        }
    };
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
            local_data: Some(dir.join("local")),
            config: Some(dir.join("config")),
            accounts: Some(dir.join("cuotai.toml")),
        };
        std::fs::create_dir_all(dir.join("local/cuotai/logs")).unwrap();
        std::fs::write(dir.join("local/cuotai/logs/cuotai.log"), b"x").unwrap();
        std::fs::write(dir.join("cuotai.toml"), b"").unwrap();
        EraseOptions {
            roots,
            include_config: false,
        }
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
        let code = erase_with(&options, &vault, false, &mut &b""[..], false, &mut out).unwrap();
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
            true,
            &mut &b""[..],
            false,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(code, 0);
    }
}
