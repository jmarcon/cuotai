//! PRD 0046 RF2/RF3/RF5: `cuotai --backup FILE [--with-history]` and
//! `cuotai --restore FILE [--check] [--yes] [--only
//! config|prefs|user|history]`, over the core's `bundle`.
//! `--restore FILE --check` is also the way to see what differs between a
//! backup and this computer (RF5.1).

use chrono::Utc;
use cuotai_core::bundle::{self, Action, Bundle, BundleError, Locations, Part, Plan};
use cuotai_core::config_file::{ConfigFile, Discovery, PathSource, ResolvedPath};
use cuotai_core::history::HistoryStore;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::l10n;

fn locations(config: Option<PathBuf>) -> Locations {
    let resolved = ConfigFile::resolve_path(
        Discovery::Cli { config },
        &cuotai_core::config_file::Locations::from_process(),
    );
    Locations::for_cli(resolved)
}

/// `--backup FILE [--json]`. Exit 0 when written, 1 on a refusal or an
/// I/O error.
pub(crate) fn backup(
    out: &Path,
    config: Option<PathBuf>,
    dev_store: bool,
    with_history: bool,
    json: bool,
    locale: &str,
) -> io::Result<u8> {
    let fail = |error: BundleError| {
        eprintln!("cuotai: {}", error.l10n().resolve(locale));
        Ok(1)
    };
    if dev_store {
        return fail(BundleError::DevStore);
    }
    let locations = locations(config);
    let now = Utc::now();
    // RF2.2: the history of the accounts as this computer names them. A
    // configuration with no data folder of its own (`--config` without
    // `CUOTAI_DATA_DIR`) has no history to carry.
    let history = if with_history && locations.data_root.is_some() {
        let file = ConfigFile::open(
            ResolvedPath {
                path: locations.config.clone(),
                source: PathSource::Argument,
            },
            true,
        );
        let store = file
            .as_ref()
            .ok()
            .and_then(|file| HistoryStore::in_data_dir(file).ok());
        match (file, store) {
            (Ok(file), Some(store)) => Some((file.config.accounts, store)),
            _ => {
                return fail(BundleError::Io {
                    path: "history".into(),
                })
            }
        }
    } else {
        None
    };
    let source = history
        .as_ref()
        .map(|(accounts, store)| bundle::HistorySource {
            store,
            accounts,
            since: now - chrono::TimeDelta::days(bundle::HISTORY_DAYS),
        });
    let bundle = match bundle::collect_with(
        &locations,
        now,
        &cuotai_core::build_info::display_version(),
        source,
    ) {
        Ok(bundle) => bundle,
        Err(error) => return fail(error),
    };
    if let Err(error) = bundle::write_file(out, &bundle.to_zip()) {
        return fail(error);
    }
    if bundle.history_bytes() > bundle::HISTORY_WARN_BYTES {
        eprintln!(
            "cuotai: {}",
            l10n::trf_in(
                locale,
                "cli.backup.history_large",
                "The backup holds {size} MiB of history; it may be slow to copy.",
                &[("size", &(bundle.history_bytes() >> 20))],
            )
        );
    }
    let mut stdout = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(&mut stdout, &bundle.manifest)?;
        writeln!(stdout)?;
    } else {
        write!(stdout, "{}", backup_text(&bundle, out, locale))?;
    }
    Ok(0)
}

fn backup_text(bundle: &Bundle, out: &Path, locale: &str) -> String {
    let mut text = l10n::trf_in(
        locale,
        "cli.backup.written",
        "Backup written to {path}. It holds no secret.",
        &[("path", &out.display())],
    ) + "\n";
    text += l10n::tr_in(locale, "cli.backup.included", "Included:");
    text.push('\n');
    for entry in &bundle.manifest.files {
        text += &format!("  + {}\n", entry.path);
    }
    text += l10n::tr_in(locale, "cli.backup.excluded", "Left out:");
    text.push('\n');
    for item in &bundle.manifest.excluded {
        text += &format!("  - {}\n", bundle::excluded_reason(item).resolve(locale));
    }
    text
}

fn action_label(action: Action, locale: &str) -> &'static str {
    match action {
        Action::Add => l10n::tr_in(locale, "cli.restore.add", "add"),
        Action::Replace => l10n::tr_in(locale, "cli.restore.replace", "replace"),
        Action::Keep => l10n::tr_in(locale, "cli.restore.keep", "keep (identical)"),
        Action::Skip => l10n::tr_in(
            locale,
            "cli.restore.skip",
            "skip (changed here after the backup)",
        ),
        Action::Merge => l10n::tr_in(locale, "cli.restore.merge", "merge"),
    }
}

pub(crate) fn plan_text(plan: &Plan, file: &Path, locale: &str) -> String {
    let mut text = l10n::trf_in(
        locale,
        "cli.restore.plan",
        "Restore plan for {path} (made {date} by Cuotai {version}):",
        &[
            ("path", &file.display()),
            ("date", &plan.created_at),
            ("version", &plan.cuotai_version),
        ],
    ) + "\n";
    if plan.items.is_empty() {
        text += l10n::tr_in(locale, "cli.restore.nothing", "Nothing to restore.");
        text.push('\n');
    }
    for item in &plan.items {
        text += &format!(
            "  {:<10} {} → {}\n",
            action_label(item.action, locale),
            item.path,
            item.target
        );
        if let Some(merge) = &item.merge {
            text += "             ";
            text += &l10n::trf_in(
                locale,
                "cli.restore.rows",
                "rows: {rows}, new here: {new}",
                &[("rows", &merge.rows), ("new", &merge.new)],
            );
            text.push('\n');
        }
        if let Some(accounts) = &item.accounts {
            text += "             ";
            text += &l10n::trf_in(
                locale,
                "cli.restore.accounts",
                "accounts: {added} added, {changed} changed, {removed} removed",
                &[
                    ("added", &accounts.added),
                    ("changed", &accounts.changed),
                    ("removed", &accounts.removed),
                ],
            );
            text.push('\n');
        }
    }
    if !plan.unplaced.is_empty() {
        text += l10n::tr_in(
            locale,
            "cli.restore.unplaced",
            "Left out (this configuration has no place for them here):",
        );
        text.push('\n');
        for path in &plan.unplaced {
            text += &format!("  - {path}\n");
        }
    }
    if !plan.newer.is_empty() {
        text += l10n::tr_in(
            locale,
            "cli.restore.newer",
            "Left untouched (a newer Cuotai wrote it, here or in the backup):",
        );
        text.push('\n');
        for path in &plan.newer {
            text += &format!("  - {path}\n");
        }
    }
    if !plan.needs.is_empty() {
        text += l10n::tr_in(
            locale,
            "cli.restore.needs",
            "Secrets do not travel; after the restore:",
        );
        text.push('\n');
        for need in &plan.needs {
            text += &format!("  • {}\n", need.resolve(locale));
        }
    }
    text
}

/// `--restore FILE`. With `--check`: print the plan, exit 0 (2 for a
/// damaged or newer backup). Otherwise apply it: additions always, each
/// replacement after a `y` in the terminal, or all of them with `--yes`.
pub(crate) fn restore(
    file: &Path,
    config: Option<PathBuf>,
    only: Option<&str>,
    check: bool,
    yes: bool,
    json: bool,
    locale: &str,
) -> io::Result<u8> {
    let fail = |error: BundleError| {
        eprintln!("cuotai: {}", error.l10n().resolve(locale));
        Ok(error.exit_code())
    };
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(_) => {
            return fail(BundleError::Io {
                path: file.display().to_string(),
            })
        }
    };
    let bundle = match Bundle::from_zip(&bytes) {
        Ok(bundle) => bundle,
        Err(error) => return fail(error),
    };
    let only = only.and_then(Part::parse);
    let var = |name: &str| std::env::var(name).ok();
    let plan = match bundle::plan(&bundle, &locations(config), only, &var) {
        Ok(plan) => plan,
        Err(error) => return fail(error),
    };
    let mut stdout = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(&mut stdout, &plan)?;
        writeln!(stdout)?;
    } else {
        write!(stdout, "{}", plan_text(&plan, file, locale))?;
    }
    if check {
        return Ok(0);
    }
    let terminal = io::stdin().is_terminal();
    if plan.has_conflicts() && !yes && !terminal {
        eprintln!(
            "cuotai: {}",
            l10n::tr_in(
                locale,
                "cli.restore.needs_terminal",
                "The restore would replace files: confirm each one in a terminal, or add --yes.",
            )
        );
        return Ok(1);
    }
    let mut input = io::stdin().lock();
    let applied = plan.apply(|item| {
        if yes {
            return true;
        }
        let question = l10n::trf_in(
            locale,
            "cli.restore.confirm",
            "Replace {path}? [y/N] ",
            &[("path", &item.target)],
        );
        let mut out = io::stdout().lock();
        let _ = write!(out, "{question}");
        let _ = out.flush();
        let mut answer = String::new();
        input.read_line(&mut answer).is_ok() && answer.trim().eq_ignore_ascii_case("y")
    });
    match applied {
        Ok(count) => {
            writeln!(
                stdout,
                "{}",
                l10n::trf_in(
                    locale,
                    "cli.restore.done",
                    "Files restored: {count}.",
                    &[("count", &count)],
                )
            )?;
            Ok(0)
        }
        Err(error) => fail(error),
    }
}
