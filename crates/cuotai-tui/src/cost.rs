//! PRD 0015 RF1/RF2 on the command line: list the local cost sources with
//! exactly what each reads, turn one on after the user read its disclosure
//! and typed the confirmation, turn one off (erasing what was derived from
//! it), erase the index, set the period and the retention, and scan in the
//! foreground. The interactive TUI keeps the index current from the core's
//! background [`CostWorker`].

use cuotai_core::cost::{
    self, CostIndex, CostWorker, Env, Limits, Period, Retention, Scanner, SettingsStore, SourceId,
    SourceState,
};
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::time::Duration;

/// What `--cost-*` asked for.
pub(crate) enum CostAction {
    Sources,
    Enable {
        source: String,
        paths: Vec<PathBuf>,
    },
    Disable(String),
    Erase,
    Scan,
    Options {
        period: Option<String>,
        retention_days: Option<u32>,
    },
}

/// The foreground scan reads everything that changed, up to a bound.
const FOREGROUND: Limits = Limits {
    max_files: 5_000,
    max_lines: 2_000_000,
    max_duration: Duration::from_secs(120),
    max_line_bytes: 8 << 20,
    max_entries: 200_000,
    max_depth: 8,
    pace: false,
};

fn stores() -> Result<(SettingsStore, CostIndex), String> {
    let error = |e: cost::CostError| e.l10n().resolve(crate::ui::tui_locale());
    Ok((
        SettingsStore::in_data_dir().map_err(error)?,
        CostIndex::in_data_dir().map_err(error)?,
    ))
}

fn source(name: &str, locale: &str) -> Result<SourceId, String> {
    SourceId::from_slug(name).ok_or_else(|| cost::unknown_source(name).resolve(locale))
}

/// RF2.2: the background scan of the interactive TUI, on a thread of its
/// own; `None` without a local data directory.
pub(crate) fn worker() -> Option<CostWorker> {
    let (settings, index) = stores().ok()?;
    Some(CostWorker::spawn(Scanner::new(
        index,
        settings,
        Env::system(),
    )))
}

/// Run `action`; returns the exit code (0 done, 2 a scan that found a
/// source on without logs or access, or none on). Errors are local (1).
pub(crate) fn run(action: CostAction, json: bool, locale: &str) -> Result<u8, String> {
    let (settings, index) = stores()?;
    let env = Env::system();
    let l10n = |error: cost::CostError| error.l10n().resolve(locale);
    let mut stdout = io::stdout().lock();
    let io_error = |error: io::Error| error.to_string();
    match action {
        CostAction::Sources => {
            let text = sources(&settings, &index, &env, json, locale)?;
            write!(stdout, "{text}").map_err(io_error)?;
            Ok(0)
        }
        CostAction::Enable {
            source: name,
            paths,
        } => {
            let source = source(&name, locale)?;
            let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();
            enable(
                &settings,
                source,
                paths,
                &env,
                interactive,
                &mut io::stdin().lock(),
                &mut stdout,
                locale,
            )?;
            Ok(0)
        }
        CostAction::Disable(name) => {
            let source = source(&name, locale)?;
            let removed = cost::turn_off(&settings, &index, source).map_err(l10n)?;
            writeln!(
                stdout,
                "{}",
                cost::disabled_text(source, removed).resolve(locale)
            )
            .map_err(io_error)?;
            Ok(0)
        }
        CostAction::Erase => {
            let removed = index.erase().map_err(l10n)?;
            writeln!(stdout, "{}", cost::erased_text(removed).resolve(locale)).map_err(io_error)?;
            Ok(0)
        }
        CostAction::Scan => {
            let scanner = Scanner::new(index, settings, env).with_limits(FOREGROUND);
            let report = scanner.scan(chrono::Utc::now()).map_err(l10n)?;
            if json {
                serde_json::to_writer_pretty(&mut stdout, &report).map_err(|e| e.to_string())?;
                writeln!(stdout).map_err(io_error)?;
            } else {
                for scan in &report.sources {
                    writeln!(stdout, "{}", scan.summary().resolve(locale)).map_err(io_error)?;
                }
            }
            let on: Vec<_> = report
                .sources
                .iter()
                .filter(|s| s.state != SourceState::Off)
                .collect();
            let read = !on.is_empty() && on.iter().all(|s| s.state == SourceState::Ready);
            Ok(if read { 0 } else { 2 })
        }
        CostAction::Options {
            period,
            retention_days,
        } => {
            if let Some(name) = period {
                let period = Period::from_slug(&name)
                    .ok_or_else(|| cost::unknown_period(&name).resolve(locale))?;
                settings.set_period(period).map_err(l10n)?;
            }
            if let Some(days) = retention_days {
                let retention = Retention::days(days).map_err(l10n)?;
                settings.set_retention(retention).map_err(l10n)?;
            }
            let saved = settings.load().map_err(l10n)?;
            writeln!(
                stdout,
                "Custo local: período {} ({}), índice mantido por {} dias.",
                saved.period.label().resolve(locale),
                saved.period.slug(),
                saved.retention().keep_days()
            )
            .map_err(io_error)?;
            Ok(0)
        }
    }
}

/// RF1.1: show the disclosure of `source` reading `paths` (the defaults
/// when empty) and turn it on only after the typed confirmation.
#[allow(clippy::too_many_arguments)]
fn enable(
    settings: &SettingsStore,
    source: SourceId,
    paths: Vec<PathBuf>,
    env: &Env,
    interactive: bool,
    input: &mut impl BufRead,
    out: &mut impl Write,
    locale: &str,
) -> Result<(), String> {
    let io_error = |error: io::Error| error.to_string();
    let disclosure = cost::Disclosure::new(source, &paths, env);
    for line in disclosure.lines() {
        let indent = if line.id == "cost.disclosure.directory" {
            "  "
        } else {
            ""
        };
        writeln!(out, "{indent}{}", line.resolve(locale)).map_err(io_error)?;
    }
    if !interactive {
        return Err(cost::needs_terminal().resolve(locale));
    }
    let answer = cost::confirm_answer().resolve(locale);
    write!(out, "{} ", cost::confirm_question(&answer).resolve(locale)).map_err(io_error)?;
    out.flush().map_err(io_error)?;
    let mut typed = String::new();
    input.read_line(&mut typed).map_err(io_error)?;
    if typed.trim().to_lowercase() != answer.to_lowercase() {
        return Err(cost::declined().resolve(locale));
    }
    settings
        .enable(source, paths, &disclosure.digest, env, chrono::Utc::now())
        .map_err(|error| error.l10n().resolve(locale))?;
    writeln!(out, "{}", cost::enabled_text(source).resolve(locale)).map_err(io_error)?;
    Ok(())
}

/// `--cost-sources`: every source, on or off, the exact directories it
/// reads (or would read) and what the index follows; no log is read.
fn sources(
    settings: &SettingsStore,
    index: &CostIndex,
    env: &Env,
    json: bool,
    locale: &str,
) -> Result<String, String> {
    let l10n = |error: cost::CostError| error.l10n().resolve(locale);
    let saved = settings.load().map_err(l10n)?;
    let mut rows = Vec::new();
    for source in SourceId::ALL {
        let disclosure = saved.disclosure(source, env);
        let indexed = index.indexed(source).map_err(l10n)?;
        let consented_at = saved
            .sources
            .get(&source)
            .and_then(|s| s.consented_at)
            .filter(|_| saved.enabled(source, env));
        rows.push((source, disclosure, indexed, consented_at));
    }
    if json {
        let sources: Vec<_> = rows
            .iter()
            .map(|(source, disclosure, indexed, consented_at)| {
                serde_json::json!({
                    "source": source.slug(),
                    "label": source.label(),
                    "enabled": consented_at.is_some(),
                    "consented_at": consented_at,
                    "custom": disclosure.custom,
                    "directories": disclosure.directories,
                    "pattern": disclosure.pattern,
                    "indexed_files": indexed.files,
                    "invalid_lines": indexed.invalid_lines,
                })
            })
            .collect();
        let value = serde_json::json!({
            "sources": sources,
            "period": saved.period.slug(),
            "retention_days": saved.retention().keep_days(),
            "index": index.path(),
            "settings": settings.path(),
        });
        return serde_json::to_string_pretty(&value)
            .map(|text| text + "\n")
            .map_err(|e| e.to_string());
    }
    let mut out =
        String::from("Custo local (estimativa só neste computador; desligado por padrão)\n");
    for (source, disclosure, indexed, consented_at) in rows {
        let state = match consented_at {
            Some(at) => format!("ligado desde {}", crate::ui::local_time(at)),
            None => "desligado".to_owned(),
        };
        out += &format!("\n{} [{}]: {state}\n", source.label(), source.slug());
        let origin = if disclosure.custom {
            "escolhidos"
        } else {
            "padrão"
        };
        out += &format!(
            "  Diretórios ({origin}), arquivos {}:\n",
            disclosure.pattern
        );
        for dir in &disclosure.directories {
            out += &format!("    {}\n", crate::ui::safe(&dir.display().to_string()));
        }
        out += &format!(
            "  No índice: {} log(s), {} linha(s) inválida(s) ignorada(s)\n",
            indexed.files, indexed.invalid_lines
        );
    }
    out += &format!(
        "\nPeríodo: {} ({}); retenção do índice: {} dias\nÍndice: {}\nPreferências: {}\n",
        saved.period.label().resolve(locale),
        saved.period.slug(),
        saved.retention().keep_days(),
        index.path().display(),
        settings.path().display()
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuotai_core::text::Localized;

    fn resolve(text: Localized) -> String {
        text.resolve("en-US")
    }

    #[test]
    fn enabling_lists_the_directories_and_needs_the_typed_answer() {
        let dir = std::env::temp_dir().join(format!("cuotai-tui-cost-{}", std::process::id()));
        let store = SettingsStore::at(dir.join("settings.json"));
        let env = Env::new(Some(dir.join("home")), []);
        let mut out = Vec::new();
        let declined = enable(
            &store,
            SourceId::CodexCli,
            Vec::new(),
            &env,
            true,
            &mut "no\n".as_bytes(),
            &mut out,
            "en-US",
        );
        assert_eq!(declined, Err(resolve(cost::declined())));
        let shown = String::from_utf8(out).unwrap();
        let sessions = env.home().unwrap().join(".codex/sessions");
        assert!(shown.contains(&sessions.display().to_string()), "{shown}");
        assert!(shown.contains("Only numbers are extracted"), "{shown}");
        assert!(!store.load().unwrap().enabled(SourceId::CodexCli, &env));

        let without_terminal = enable(
            &store,
            SourceId::CodexCli,
            Vec::new(),
            &env,
            false,
            &mut "yes\n".as_bytes(),
            &mut Vec::new(),
            "en-US",
        );
        assert_eq!(without_terminal, Err(resolve(cost::needs_terminal())));
        assert!(!store.load().unwrap().enabled(SourceId::CodexCli, &env));

        let mut out = Vec::new();
        enable(
            &store,
            SourceId::CodexCli,
            vec![PathBuf::from("/srv/codex-logs")],
            &env,
            true,
            &mut "yes\n".as_bytes(),
            &mut out,
            "en-US",
        )
        .unwrap();
        let saved = store.load().unwrap();
        assert!(saved.enabled(SourceId::CodexCli, &env));
        assert_eq!(
            saved.roots(SourceId::CodexCli, &env),
            [PathBuf::from("/srv/codex-logs")]
        );
        assert!(!saved.enabled(SourceId::ClaudeCode, &env));
        assert!(String::from_utf8(out).unwrap().contains("/srv/codex-logs"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
