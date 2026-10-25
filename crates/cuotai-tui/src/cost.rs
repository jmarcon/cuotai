//! PRD 0015 RF1–RF4 on the command line: list the local cost sources with
//! exactly what each reads, turn one on after the user read its disclosure
//! and typed the confirmation, turn one off (erasing what was derived from
//! it), erase the index, set the period and the retention, scan in the
//! foreground, show the prices the estimate uses (the embedded table under
//! the user's `pricing.json`) and show the estimate itself (`--cost`). The
//! interactive TUI and `--serve` keep the index current from the core's
//! background [`CostWorker`] ([`LocalCost`]); the TUI shows the core's
//! [`CostSummary`] in the account details and the exports carry it as
//! `local_cost`.

use cuotai_core::cost::{
    self, CostIndex, CostSummary, CostWorker, Env, Limits, Period, PriceOrigin, Prices,
    PricingFile, Retention, ScanReport, Scanner, SettingsStore, SourceId, SourceState, Span,
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
    Prices,
    /// RF4.2 `--cost [--days N]`.
    Usage {
        days: Option<u32>,
    },
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

/// RF2.2/RF4: this computer's cost index as the interactive TUI and
/// `--serve` read it, kept current by the core's background scan on a
/// thread of its own (it ends with the last clone).
#[derive(Clone)]
pub(crate) struct LocalCost {
    scanner: Scanner,
    pricing: PricingFile,
    worker: CostWorker,
}

impl LocalCost {
    /// Start the background scan; `None` without a local data directory.
    pub(crate) fn start() -> Option<Self> {
        let (settings, index) = stores().ok()?;
        let scanner = Scanner::new(index, settings, Env::system());
        Some(Self {
            worker: CostWorker::spawn(scanner.clone()),
            pricing: PricingFile::in_data_dir().ok()?,
            scanner,
        })
    }

    /// RF4: the saved period at `now`, over the background scan's last
    /// report. Reads the index, never a log.
    pub(crate) fn summary(&self, now: chrono::DateTime<chrono::Utc>) -> Option<CostSummary> {
        let last = self.worker.status().last;
        self.scanner
            .summary(&self.pricing, last.as_ref(), None, now)
            .ok()
    }

    /// RF4.3: the export's `local_cost`, `None` while every source is off.
    pub(crate) fn export(&self, now: chrono::DateTime<chrono::Utc>) -> Option<CostSummary> {
        self.summary(now).filter(CostSummary::any_on)
    }
}

/// RF4.3: `local_cost` of a one-shot export (`--json-v1`): a scan bounded by
/// the background limits, then the summary; `None` while every source is
/// off (nothing is read) or without a local data directory.
pub(crate) fn one_shot_export() -> Option<CostSummary> {
    let (settings, index) = stores().ok()?;
    let scanner = Scanner::new(index, settings, Env::system());
    let on = scanner
        .settings()
        .load()
        .ok()?
        .enabled_sources(scanner.env());
    if on.is_empty() {
        return None;
    }
    let report = scanner.scan(chrono::Utc::now()).ok();
    let pricing = PricingFile::in_data_dir().ok()?;
    scanner
        .summary(&pricing, report.as_ref(), None, chrono::Utc::now())
        .ok()
        .filter(CostSummary::any_on)
}

/// 0 when some source is on and every one that is on was read; 2 when none
/// is on or one has no logs or no access.
fn scan_code(report: &ScanReport) -> u8 {
    let on: Vec<_> = report
        .sources
        .iter()
        .filter(|s| s.state != SourceState::Off)
        .collect();
    let read = !on.is_empty() && on.iter().all(|s| s.state == SourceState::Ready);
    if read {
        0
    } else {
        2
    }
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
        CostAction::Prices => {
            let file = PricingFile::in_data_dir().map_err(l10n)?;
            let (text, code) = prices(&file, json, locale)?;
            write!(stdout, "{text}").map_err(io_error)?;
            Ok(code)
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
            Ok(scan_code(&report))
        }
        CostAction::Usage { days } => {
            let span = days.map(Span::days).transpose().map_err(l10n)?;
            let pricing = PricingFile::in_data_dir().map_err(l10n)?;
            let scanner = Scanner::new(index, settings, env).with_limits(FOREGROUND);
            let now = chrono::Utc::now();
            let report = scanner.scan(now).map_err(l10n)?;
            let summary = scanner
                .summary(&pricing, Some(&report), span, now)
                .map_err(l10n)?;
            if json {
                serde_json::to_writer_pretty(&mut stdout, &summary).map_err(|e| e.to_string())?;
                writeln!(stdout).map_err(io_error)?;
            } else {
                write!(stdout, "{}", usage_text(&summary, locale)).map_err(io_error)?;
            }
            Ok(scan_code(&report))
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

/// RF3.1/RF3.2 `--cost-prices`: the prices in effect, model by model, with
/// the embedded table's date and source and the state of the user's
/// `pricing.json` (missing, used, or ignored as a whole with exit 1 when
/// invalid). Reads no log.
fn prices(file: &PricingFile, json: bool, locale: &str) -> Result<(String, u8), String> {
    let (prices, error) = Prices::with_user_file(file);
    let code = u8::from(error.is_some());
    let notice = cost::estimate_notice().resolve(locale);
    let ignored = error
        .as_ref()
        .map(|e| cost::pricing_ignored(e).resolve(locale));
    let rows = prices.rows();
    if json {
        let value = serde_json::json!({
            "currency": cost::CURRENCY,
            "unit": "usd_per_million_tokens",
            "notice": notice,
            "tables": prices.info(),
            "pricing_file": {
                "path": file.path(),
                "used": prices.user().is_some(),
                "error": ignored,
            },
            "models": rows,
        });
        return serde_json::to_string_pretty(&value)
            .map(|text| (text + "\n", code))
            .map_err(|e| e.to_string());
    }
    let source = |source: &Option<String>| source.as_deref().unwrap_or("—").to_owned();
    let mut out =
        format!("Preços da estimativa de custo local (USD por milhão de tokens)\n{notice}\n\n");
    for info in prices.info() {
        out += &match info.origin {
            PriceOrigin::Embedded => format!(
                "Tabela embutida: preços de {}; fonte: {}\n",
                info.effective,
                source(&info.source)
            ),
            PriceOrigin::User => format!(
                "Sua tabela ({}): preços de {}; fonte: {}; {} modelo(s), que valem sobre a embutida\n",
                file.path().display(),
                info.effective,
                source(&info.source),
                prices.user().map_or(0, |table| table.models.len())
            ),
        };
    }
    match &ignored {
        Some(reason) => {
            out += &format!(
                "Sua tabela ({}): ignorada. {reason}\n",
                file.path().display()
            )
        }
        None if prices.user().is_none() => {
            out += &format!(
            "Sua tabela ({}): não existe; crie-a no mesmo formato para revisar ou trocar preços\n",
            file.path().display()
        )
        }
        None => {}
    }
    let width = rows
        .iter()
        .map(|row| row.model.len())
        .max()
        .unwrap_or(0)
        .max(6);
    out += &format!(
        "\n{:<width$}  {:>9}  {:>9}  {:>13}  {:>14}  {:>17}  Origem\n",
        "Modelo", "Entrada", "Saída", "Leitura cache", "Gravação cache", "Gravação cache 1h"
    );
    for row in &rows {
        let origin = match row.origin {
            PriceOrigin::Embedded => "embutida",
            PriceOrigin::User => "sua tabela",
        };
        out += &format!(
            "{:<width$}  {:>9}  {:>9}  {:>13}  {:>14}  {:>17}  {origin}\n",
            row.model,
            row.rates.input.to_string(),
            row.rates.output.to_string(),
            row.rates.cache_read.to_string(),
            row.rates.cache_write.to_string(),
            row.rates
                .cache_write_1h
                .map_or_else(|| "—".to_owned(), |rate| rate.to_string())
        );
    }
    out +=
        "\nUm modelo fora da tabela tem custo desconhecido, nunca zero. Uma data no fim do nome \
            (-20250929) usa o preço do nome sem ela.\n";
    Ok((out, code))
}

/// How a row of a source's section reads, for styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Row {
    /// Why the numbers are missing or incomplete.
    Note,
    /// Today, yesterday or the span.
    Amount,
    /// "By model", "By day".
    Heading,
    /// One model or one day.
    Item,
}

/// RF4.2: the rows of one source, shared by `--cost` and the TUI's account
/// details: its note, today, yesterday and the span, then the breakdown by
/// model and by day (the days with usage). A source that is off has only
/// its note.
pub(crate) fn source_rows(
    source: &cost::SourceCost,
    span: &cost::SpanView,
) -> Vec<(Row, cuotai_core::text::Localized)> {
    let mut rows: Vec<_> = source.note.iter().map(|n| (Row::Note, n.clone())).collect();
    let Some(usage) = &source.usage else {
        return rows;
    };
    rows.extend([
        (Row::Amount, usage.today.line(Period::Today.label())),
        (Row::Amount, usage.yesterday.line(cost::yesterday_label())),
        (Row::Amount, usage.period.line(span.label.clone())),
    ]);
    if !usage.period.models.is_empty() {
        rows.push((Row::Heading, cost::by_model_heading(span)));
        rows.extend(usage.period.models.iter().map(|m| (Row::Item, m.line())));
    }
    let used: Vec<_> = usage.days.iter().filter(|d| d.tokens.total() > 0).collect();
    if !used.is_empty() {
        rows.push((Row::Heading, cost::by_day_heading()));
        rows.extend(used.into_iter().map(|d| (Row::Item, d.line())));
    }
    rows
}

/// RF4.2 `--cost` in text: the span with its days, the notice, an ignored
/// `pricing.json`, then each source's rows (a source that is off, a single
/// line) and how to turn one on when none is.
fn usage_text(summary: &CostSummary, locale: &str) -> String {
    let span = &summary.span;
    let range = match span.first_day {
        Some(first) => format!("{first} → {}", span.last_day),
        None => format!("… → {}", span.last_day),
    };
    let mut out = format!(
        "{} ({range})\n{}\n",
        span.label.resolve(locale),
        summary.notice.resolve(locale)
    );
    if let Some(error) = &summary.pricing_error {
        out += &format!("{}\n", error.resolve(locale));
    }
    for source in &summary.sources {
        out.push('\n');
        if source.usage.is_some() {
            out += &format!("{}\n", cost::heading(source.source).resolve(locale));
        }
        for (row, text) in source_rows(source, span) {
            let indent = match row {
                _ if source.usage.is_none() => "",
                Row::Note | Row::Amount | Row::Heading => "  ",
                Row::Item => "    ",
            };
            out += &format!("{indent}{}\n", crate::ui::safe(&text.resolve(locale)));
        }
    }
    if !summary.any_on() {
        out += &format!("\n{}\n", cost::enable_hint().resolve(locale));
    }
    out
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
        let sessions = env.home().unwrap().join(".codex").join("sessions");
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

        let custom = dir.join("codex-logs");
        let mut out = Vec::new();
        enable(
            &store,
            SourceId::CodexCli,
            vec![custom.clone()],
            &env,
            true,
            &mut "yes\n".as_bytes(),
            &mut out,
            "en-US",
        )
        .unwrap();
        let saved = store.load().unwrap();
        assert!(saved.enabled(SourceId::CodexCli, &env));
        let roots = saved.roots(SourceId::CodexCli, &env);
        assert_eq!(roots.as_slice(), std::slice::from_ref(&custom));
        assert!(!saved.enabled(SourceId::ClaudeCode, &env));
        assert!(String::from_utf8(out)
            .unwrap()
            .contains(custom.to_string_lossy().as_ref()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prices_show_the_users_table_over_the_embedded_one_and_fail_on_an_invalid_one() {
        let dir = std::env::temp_dir().join(format!("cuotai-tui-prices-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = PricingFile::at(dir.join("pricing.json"));
        let model = |value: &serde_json::Value, name: &str| {
            value["models"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["model"] == name)
                .cloned()
        };

        let (text, code) = prices(&file, true, "en-US").unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(code, 0);
        assert_eq!(value["pricing_file"]["used"], false);
        assert_eq!(
            value["notice"],
            "Estimate at API prices, not a charge; amounts in USD."
        );
        assert_eq!(
            model(&value, "claude-sonnet-4-5").unwrap()["origin"],
            "embedded"
        );

        std::fs::write(
            file.path(),
            r#"{"effective": "2026-10-15", "source": "contract", "models": {
                "claude-sonnet-4-5": {"input": 1, "output": 5, "cache_read": 0.1, "cache_write": 1.25}}}"#,
        )
        .unwrap();
        let (text, code) = prices(&file, true, "en-US").unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(code, 0);
        let sonnet = model(&value, "claude-sonnet-4-5").unwrap();
        assert_eq!(
            (&sonnet["origin"], &sonnet["rates"]["input"]),
            (&"user".into(), &1.0.into())
        );
        assert_eq!(value["tables"][1]["effective"], "2026-10-15");
        let (text, _) = prices(&file, false, "en-US").unwrap();
        assert!(
            text.contains("preços de 2026-10-15; fonte: contract; 1 modelo(s)"),
            "{text}"
        );

        std::fs::write(
            file.path(),
            r#"{"currency": "EUR", "effective": "2026-10-15", "models": {}}"#,
        )
        .unwrap();
        let (text, code) = prices(&file, true, "en-US").unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(code, 1);
        assert_eq!(value["pricing_file"]["used"], false);
        assert_eq!(
            value["pricing_file"]["error"],
            "pricing.json only accepts prices in USD. Only the embedded table is used until the file is fixed."
        );
        assert_eq!(
            model(&value, "claude-sonnet-4-5").unwrap()["origin"],
            "embedded"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
