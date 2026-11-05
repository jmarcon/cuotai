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

use crate::dashboard::{cells, pad_end, pad_start};
use crate::l10n::{tr, tr_in, trf, trf_in};
use cuotai_core::cost::{
    self, Breakdown, CostIndex, CostSummary, CostWorker, DayZone, Env, Limits, Period, PriceOrigin,
    Prices, PricingFile, Retention, ScanReport, Scanner, SettingsStore, SourceId, SourceState,
    Span,
};
use cuotai_core::{Account, Provider};
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
    /// PRD 0030 RF1.1 `--cost-plan ACCOUNT PRICE CURRENCY [DAY]`.
    Plan {
        account: String,
        price: String,
        currency: String,
        cycle_day: Option<String>,
    },
    /// `--cost-plan-clear ACCOUNT`.
    PlanClear(String),
    /// PRD 0030 RF6.3 `--cost-currency CODE RATE` / `off`.
    Currency {
        code: String,
        rate: Option<String>,
    },
    /// PRD 0030 RF4.2 `--cost-prices --template MODEL…`.
    PricesTemplate(Vec<String>),
    /// RF4.2 `--cost [--days N]`; PRD 0042 RF5.1 `--by BY [--top N | --all]`
    /// (`top` is `None` with `--all`).
    Usage {
        days: Option<u32>,
        by: Option<String>,
        top: Option<usize>,
    },
    /// PRD 0042 RF5.2 `--cost-live [--source S] [--watch-interval S]`.
    Live {
        source: Option<String>,
        interval: Option<u64>,
    },
    /// PRD 0042 RF2.2/RF4.2 `--cost-label TARGET PREFIX [LABEL] [--clear]`.
    Label {
        target: String,
        prefix: String,
        label: Option<String>,
        clear: bool,
    },
    /// PRD 0042 RF4.1 `--cost-hide` / `--cost-unhide PREFIX`.
    Hide {
        prefix: String,
        hidden: bool,
    },
    /// PRD 0042 RF3.1 `--cost-block-hours SOURCE HOURS`.
    BlockHours {
        source: String,
        hours: String,
    },
    /// PRD 0042 RF4.3 `--cost-folder-hint SOURCE on|off`.
    FolderHint {
        source: String,
        state: String,
    },
    Options {
        period: Option<String>,
        retention_days: Option<u32>,
    },
}

/// PRD 0042 RF5.1: the rows `--by session|project` shows without `--top`.
pub(crate) const DEFAULT_TOP: usize = 20;

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

    /// What the interactive TUI shows: [`Self::summary`] plus PRD 0030's
    /// plan lines for `accounts` (RF1) and the display currency (RF6).
    /// Plans still saved under a legacy `provider:name` key move to the
    /// account id first. Never part of the export.
    pub(crate) fn panel(
        &self,
        accounts: &[Account],
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<CostPanel> {
        let summary = self.summary(now)?;
        let _ = self.scanner.settings().migrate_plans(accounts);
        let display = self
            .scanner
            .settings()
            .load()
            .ok()
            .and_then(|saved| saved.display_currency);
        let plans = self
            .scanner
            .plan_values(&self.pricing, accounts, now)
            .unwrap_or_default();
        Some(CostPanel {
            summary,
            plans,
            display,
        })
    }

    /// RF4.3: the export's `local_cost`, `None` while every source is off.
    pub(crate) fn export(&self, now: chrono::DateTime<chrono::Utc>) -> Option<CostSummary> {
        self.summary(now).filter(CostSummary::any_on)
    }
}

/// The local cost as the TUI draws it: the core's summary, the plan values
/// of the configured accounts and the typed display currency.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CostPanel {
    pub(crate) summary: CostSummary,
    pub(crate) plans: Vec<cost::PlanValue>,
    pub(crate) display: Option<cost::DisplayCurrency>,
}

impl CostPanel {
    /// A panel with neither plans nor a second currency.
    #[cfg(test)]
    pub(crate) fn of(summary: CostSummary) -> Self {
        Self {
            summary,
            plans: Vec::new(),
            display: None,
        }
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
pub(crate) fn run(
    action: CostAction,
    json: bool,
    locale: &str,
    configured: Option<Vec<Account>>,
) -> Result<u8, String> {
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
        CostAction::Plan {
            account,
            price,
            currency,
            cycle_day,
        } => {
            let number = |text: &str| text.trim().replace(',', ".").parse::<f64>();
            let plan = number(&price)
                .map_err(|_| cost::CostError::InvalidPlan)
                .and_then(|price| {
                    let day = cycle_day
                        .as_deref()
                        .map(|day| day.trim().parse::<u8>())
                        .transpose()
                        .map_err(|_| cost::CostError::InvalidPlan)?;
                    cost::PlanSetting::new(price, &currency, day)
                })
                .map_err(l10n)?;
            let key = plan_key(configured.as_deref(), &account);
            settings.set_plan(&key, Some(plan)).map_err(l10n)?;
            writeln!(
                stdout,
                "{}",
                trf(
                    "cost.plan.saved",
                    "Plan of {account} saved on this computer.",
                    &[("account", &account)],
                )
            )
            .map_err(io_error)?;
            Ok(0)
        }
        CostAction::PlanClear(account) => {
            // Both the id and the legacy key, so no older plan survives.
            if let Some(found) = find_account(configured.as_deref(), &account) {
                settings.set_plan(&found.key(), None).map_err(l10n)?;
                settings.set_plan(&found.state_key(), None).map_err(l10n)?;
            }
            settings.set_plan(&account, None).map_err(l10n)?;
            writeln!(
                stdout,
                "{}",
                trf(
                    "cost.plan.removed",
                    "Plan of {account} removed.",
                    &[("account", &account)],
                )
            )
            .map_err(io_error)?;
            Ok(0)
        }
        CostAction::Currency { code, rate } => {
            if code.eq_ignore_ascii_case("off") && rate.is_none() {
                settings.set_display_currency(None).map_err(l10n)?;
                writeln!(
                    stdout,
                    "{}",
                    tr("cost.currency.off", "Second currency turned off; USD only.")
                )
                .map_err(io_error)?;
                return Ok(0);
            }
            let today = chrono::Local::now().date_naive();
            let rate = rate
                .ok_or(cost::CostError::InvalidRate)
                .and_then(|rate| {
                    rate.trim()
                        .replace(',', ".")
                        .parse::<f64>()
                        .map_err(|_| cost::CostError::InvalidRate)
                })
                .and_then(|rate| cost::DisplayCurrency::new(&code, rate, today))
                .map_err(l10n)?;
            let line = format!("1 USD = {} {}", rate.rate, rate.code);
            settings.set_display_currency(Some(rate)).map_err(l10n)?;
            writeln!(
                stdout,
                "{}",
                trf(
                    "cost.currency.saved",
                    "Second currency saved: {line} (display only; the totals stay in USD).",
                    &[("line", &line)],
                )
            )
            .map_err(io_error)?;
            Ok(0)
        }
        CostAction::PricesTemplate(models) => {
            let today = chrono::Local::now().date_naive();
            let text = cuotai_core::cost::PriceTable::template(&models, today)
                .map_err(|error| error.l10n().resolve(locale))?;
            write!(stdout, "{text}").map_err(io_error)?;
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
            Ok(scan_code(&report))
        }
        CostAction::Usage { days, by, top } => {
            let span = days.map(Span::days).transpose().map_err(l10n)?;
            let by = match by.as_deref() {
                None => Breakdown::Day,
                Some(name) => Breakdown::from_slug(name)
                    .ok_or_else(|| cost::unknown_breakdown(name).resolve(locale))?,
            };
            let pricing = PricingFile::in_data_dir().map_err(l10n)?;
            let scanner = Scanner::new(index, settings, env).with_limits(FOREGROUND);
            let now = chrono::Utc::now();
            let report = scanner.scan(now).map_err(l10n)?;
            if matches!(
                by,
                Breakdown::Session | Breakdown::Block | Breakdown::Project
            ) {
                let span = span.unwrap_or(Span::Period(
                    scanner.settings().load().map_err(l10n)?.period,
                ));
                let text = breakdown_text(&scanner, &pricing, by, span, top, json, now, locale)?;
                write!(stdout, "{text}").map_err(io_error)?;
                return Ok(scan_code(&report));
            }
            let summary = scanner
                .summary(&pricing, Some(&report), span, now)
                .map_err(l10n)?;
            if json {
                serde_json::to_writer_pretty(&mut stdout, &summary).map_err(|e| e.to_string())?;
                writeln!(stdout).map_err(io_error)?;
            } else if by == Breakdown::Model {
                write!(stdout, "{}", models_text(&summary, locale)).map_err(io_error)?;
            } else {
                if let Some(configured) = configured.as_deref() {
                    // PRD 0030 RF1.1: plans typed under `provider:name`
                    // move to the account id.
                    let _ = scanner.settings().migrate_plans(configured);
                }
                let saved = scanner.settings().load().unwrap_or_default();
                let accounts = configured
                    .clone()
                    .unwrap_or_else(|| planned_accounts(&saved.plans));
                let plans = scanner
                    .plan_values(&pricing, &accounts, now)
                    .unwrap_or_default();
                write!(
                    stdout,
                    "{}",
                    usage_text(&summary, saved.display_currency.as_ref(), &plans, locale)
                )
                .map_err(io_error)?;
            }
            Ok(scan_code(&report))
        }
        CostAction::Live {
            source: name,
            interval,
        } => {
            let only = name.as_deref().map(|n| source(n, locale)).transpose()?;
            if let Some(seconds) = interval {
                if !(2..=3600).contains(&seconds) {
                    return Err(tr_in(
                        locale,
                        "cost.live.interval_bounds",
                        "--watch-interval ranges from 2 to 3600 seconds.",
                    )
                    .to_owned());
                }
            }
            drop(stdout);
            let pricing = PricingFile::in_data_dir().map_err(l10n)?;
            let scanner = Scanner::new(index, settings, env).with_limits(FOREGROUND);
            live_loop(&scanner, &pricing, only, interval, json, locale)
        }
        CostAction::Label {
            target,
            prefix,
            label,
            clear,
        } => {
            let target = cost::LabelTarget::from_slug(&target).ok_or_else(|| {
                trf_in(
                    locale,
                    "cost.label.target",
                    "Unknown label target: {name}. Targets: session, project.",
                    &[("name", &target)],
                )
            })?;
            let label = if clear {
                None
            } else {
                Some(label.ok_or_else(|| cost::CostError::InvalidLabel.l10n().resolve(locale))?)
            };
            let digest = index
                .set_label(target, &prefix, label.as_deref())
                .map_err(l10n)?;
            writeln!(
                stdout,
                "{}",
                cost::label_saved_text(&digest, label.is_some()).resolve(locale)
            )
            .map_err(io_error)?;
            Ok(0)
        }
        CostAction::Hide { prefix, hidden } => {
            let digest = index.set_hidden(&prefix, hidden).map_err(l10n)?;
            writeln!(
                stdout,
                "{}",
                cost::hidden_saved_text(&digest, hidden).resolve(locale)
            )
            .map_err(io_error)?;
            Ok(0)
        }
        CostAction::BlockHours {
            source: name,
            hours,
        } => {
            let source = source(&name, locale)?;
            let hours = hours
                .trim()
                .parse::<u8>()
                .map_err(|_| cost::CostError::BlockHours)
                .map_err(l10n)?;
            settings.set_block_hours(source, hours).map_err(l10n)?;
            writeln!(
                stdout,
                "{}",
                cost::block_hours_text(source, hours).resolve(locale)
            )
            .map_err(io_error)?;
            Ok(0)
        }
        CostAction::FolderHint {
            source: name,
            state,
        } => {
            let source = source(&name, locale)?;
            let on = match state.trim().to_ascii_lowercase().as_str() {
                "on" => true,
                "off" => false,
                _ => {
                    return Err(tr_in(locale, "cost.folder_hint.state", "Use on or off.").to_owned())
                }
            };
            let removed = cost::set_folder_hint(&settings, &index, source, on).map_err(l10n)?;
            writeln!(
                stdout,
                "{}",
                cost::folder_hint_text(source, on, removed).resolve(locale)
            )
            .map_err(io_error)?;
            Ok(0)
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
                "{}",
                trf_in(
                    locale,
                    "cost.options.saved",
                    "Local cost: period {period} ({slug}), index kept for {days} days.",
                    &[
                        ("period", &saved.period.label().resolve(locale)),
                        ("slug", &saved.period.slug()),
                        ("days", &saved.retention().keep_days()),
                    ],
                )
            )
            .map_err(io_error)?;
            Ok(0)
        }
    }
}

/// The configured account `name` designates: its id or its
/// `provider:name` key.
fn find_account<'a>(accounts: Option<&'a [Account]>, name: &str) -> Option<&'a Account> {
    accounts?
        .iter()
        .find(|account| account.state_key() == name || account.key() == name)
}

/// PRD 0029 RF2: the settings key of the plan of `name`: the account id
/// when the configuration names it, else `name` as typed (a legacy
/// `provider:name` key, matched by the core's fallback).
fn plan_key(accounts: Option<&[Account]>, name: &str) -> String {
    find_account(accounts, name).map_or_else(|| name.to_owned(), Account::state_key)
}

/// Without a configuration: the accounts the saved plans name
/// (`provider:name`), enough for
/// [`cost::plan_values`]; plans of extension accounts are not matched.
fn planned_accounts(plans: &std::collections::BTreeMap<String, cost::PlanSetting>) -> Vec<Account> {
    plans
        .keys()
        .filter_map(|key| {
            let (provider, name) = key.split_once(':')?;
            Some(Account {
                group: Default::default(),
                id: None,
                name: name.to_owned(),
                provider: Provider::from_slug(provider)?,
                credential_env: None,
                oauth: None,
                session: None,
                params: Default::default(),
                refresh_seconds: None,
                budgets: Vec::new(),
                floors: Vec::new(),
                breakdown: Vec::new(),
            })
        })
        .collect()
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
    if !cuotai_core::text::answer_matches(&typed, &answer) {
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
    let mut out = format!(
        "{}\n",
        tr_in(
            locale,
            "cost.sources.heading",
            "Local cost (an estimate on this computer only; off by default)",
        )
    );
    for (source, disclosure, indexed, consented_at) in rows {
        let state = match consented_at {
            Some(at) => trf_in(
                locale,
                "cost.sources.on_since",
                "on since {at}",
                &[("at", &crate::ui::local_time(at))],
            ),
            None => tr_in(locale, "cost.sources.off", "off").to_owned(),
        };
        out += &format!("\n{} [{}]: {state}\n", source.label(), source.slug());
        let origin = if disclosure.custom {
            tr_in(locale, "cost.sources.dirs.custom", "chosen")
        } else {
            tr_in(locale, "cost.sources.dirs.default", "default")
        };
        out += &trf_in(
            locale,
            "cost.sources.dirs",
            "  Directories ({origin}), files {pattern}:",
            &[("origin", &origin), ("pattern", &disclosure.pattern)],
        );
        out += "\n";
        for dir in &disclosure.directories {
            out += &format!("    {}\n", crate::ui::safe(&dir.display().to_string()));
        }
        out += &trf_in(
            locale,
            "cost.sources.indexed",
            "  In the index: {files} log(s), {invalid} invalid line(s) ignored",
            &[
                ("files", &indexed.files),
                ("invalid", &indexed.invalid_lines),
            ],
        );
        out += "\n";
    }
    out += "\n";
    out += &trf_in(
        locale,
        "cost.sources.footer",
        "Period: {period} ({slug}); index retention: {days} days\nIndex: {index}\nPreferences: {settings}",
        &[
            ("period", &saved.period.label().resolve(locale)),
            ("slug", &saved.period.slug()),
            ("days", &saved.retention().keep_days()),
            ("index", &index.path().display()),
            ("settings", &settings.path().display()),
        ],
    );
    out += "\n";
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
    let today = chrono::Local::now().date_naive();
    let ages: Vec<Option<u32>> = prices.info().iter().map(|i| i.age(today)).collect();
    if json {
        let value = serde_json::json!({
            "currency": cost::CURRENCY,
            "unit": "usd_per_million_tokens",
            "notice": notice,
            "tables": prices.info(),
            "prices_age_days": ages.iter().flatten().max(),
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
    let mut out = format!(
        "{}\n{notice}\n\n",
        tr_in(
            locale,
            "cost.prices.heading",
            "Prices of the local cost estimate (USD per million tokens)",
        )
    );
    for info in prices.info() {
        out += &match info.origin {
            PriceOrigin::Embedded => trf_in(
                locale,
                "cost.prices.embedded",
                "Built-in table: prices of {date}; source: {source}",
                &[
                    ("date", &info.effective),
                    ("source", &source(&info.source)),
                ],
            ),
            PriceOrigin::User => trf_in(
                locale,
                "cost.prices.user",
                "Your table ({path}): prices of {date}; source: {source}; {count} model(s), which take precedence over the built-in one",
                &[
                    ("path", &file.path().display()),
                    ("date", &info.effective),
                    ("source", &source(&info.source)),
                    ("count", &prices.user().map_or(0, |table| table.models.len())),
                ],
            ),
        };
        out += "\n";
    }
    if let Some(days) = ages
        .iter()
        .flatten()
        .max()
        .filter(|d| **d >= cost::PRICE_AGE_NOTICE_DAYS)
    {
        out += &format!("{}\n", cost::prices_age_text(*days).resolve(locale));
    }
    match &ignored {
        Some(reason) => {
            out += &trf_in(
                locale,
                "cost.prices.user_ignored",
                "Your table ({path}): ignored. {reason}",
                &[("path", &file.path().display()), ("reason", reason)],
            );
            out += "\n";
        }
        None if prices.user().is_none() => {
            out += &trf_in(
                locale,
                "cost.prices.user_missing",
                "Your table ({path}): does not exist; create it in the same format to review or change prices",
                &[("path", &file.path().display())],
            );
            out += "\n";
        }
        None => {}
    }
    // Columns are measured in terminal cells (PRD 0035 RF6.1): a translated
    // header in CJK takes two cells per character, which `{:>N}` ignores.
    let header = [
        tr_in(locale, "cost.prices.col.model", "Model").to_owned(),
        tr_in(locale, "cost.prices.col.input", "Input").to_owned(),
        tr_in(locale, "cost.prices.col.output", "Output").to_owned(),
        tr_in(locale, "cost.prices.col.cache_read", "Cache read").to_owned(),
        tr_in(locale, "cost.prices.col.cache_write", "Cache write").to_owned(),
        tr_in(locale, "cost.prices.col.cache_write_1h", "Cache write 1h").to_owned(),
        tr_in(locale, "cost.prices.col.origin", "Origin").to_owned(),
    ];
    let body: Vec<[String; 7]> = rows
        .iter()
        .map(|row| {
            let origin = match row.origin {
                PriceOrigin::Embedded => tr_in(locale, "cost.prices.origin.embedded", "built-in"),
                PriceOrigin::User => tr_in(locale, "cost.prices.origin.user", "your table"),
            };
            [
                row.model.clone(),
                row.rates.input.to_string(),
                row.rates.output.to_string(),
                row.rates.cache_read.to_string(),
                row.rates.cache_write.to_string(),
                row.rates
                    .cache_write_1h
                    .map_or_else(|| "—".to_owned(), |rate| rate.to_string()),
                origin.to_owned(),
            ]
        })
        .collect();
    let mut widths = [6, 9, 9, 13, 14, 17, 0];
    for line in std::iter::once(&header).chain(&body) {
        for (width, cell) in widths.iter_mut().zip(line) {
            *width = (*width).max(cells(cell));
        }
    }
    let render = |line: &[String; 7]| {
        let mut text = pad_end(&line[0], widths[0]);
        for (cell, width) in line[1..6].iter().zip(&widths[1..6]) {
            text += "  ";
            text += &pad_start(cell, *width);
        }
        text += "  ";
        text += &line[6];
        text + "\n"
    };
    out += "\n";
    out += &render(&header);
    for line in &body {
        out += &render(line);
    }
    out += "\n";
    out += tr_in(
        locale,
        "cost.prices.footer",
        "A model outside the table has an unknown cost, never zero. A date at the end of the name \
         (-20250929) uses the price of the name without it.",
    );
    out += "\n";
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
    summary: &CostSummary,
    source: &cost::SourceCost,
) -> Vec<(Row, cuotai_core::text::Localized)> {
    let span = &summary.span;
    let mut rows: Vec<_> = source.note.iter().map(|n| (Row::Note, n.clone())).collect();
    let Some(usage) = &source.usage else {
        return rows;
    };
    rows.extend([
        (Row::Amount, usage.today.line(Period::Today.label())),
        (Row::Amount, usage.yesterday.line(cost::yesterday_label())),
        (Row::Amount, usage.period.line(span.label.clone())),
    ]);
    // PRD 0042 RF6.2: the active block, only with a burn rate.
    if let Some(live) = summary
        .live
        .iter()
        .find(|l| l.block.source == source.source)
    {
        rows.extend(live.line.iter().map(|line| (Row::Amount, line.clone())));
        rows.extend(live.projection.iter().map(|p| (Row::Amount, p.clone())));
    }
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
fn usage_text(
    summary: &CostSummary,
    display: Option<&cost::DisplayCurrency>,
    plans: &[cost::PlanValue],
    locale: &str,
) -> String {
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
    for note in summary.prices_age.iter().chain(&summary.unpriced) {
        out += &format!("{}\n", crate::ui::safe(&note.resolve(locale)));
    }
    for source in &summary.sources {
        out.push('\n');
        if source.usage.is_some() {
            out += &format!("{}\n", cost::heading(source.source).resolve(locale));
        }
        for (row, text) in source_rows(summary, source) {
            let indent = match row {
                _ if source.usage.is_none() => "",
                Row::Note | Row::Amount | Row::Heading => "  ",
                Row::Item => "    ",
            };
            out += &format!("{indent}{}\n", crate::ui::safe(&text.resolve(locale)));
        }
    }
    if let (Some(display), Some(total)) = (display, summary.total.total) {
        let today = summary.span.last_day;
        out += &format!(
            "\n{}: {}\n",
            summary.span.label.resolve(locale),
            display.text(total, today).resolve(locale)
        );
    }
    if !plans.is_empty() {
        out.push('\n');
        for plan in plans {
            out += &format!("{}\n", crate::ui::safe(&plan.text.resolve(locale)));
        }
    }
    if !summary.any_on() {
        out += &format!("\n{}\n", cost::enable_hint().resolve(locale));
    }
    out
}

/// PRD 0042 RF5.1 `--cost --by model` in text: per source on, the span's
/// amount and its models.
fn models_text(summary: &CostSummary, locale: &str) -> String {
    let span = &summary.span;
    let mut out = format!(
        "{}\n{}\n",
        cost::by_heading(Breakdown::Model, span).resolve(locale),
        summary.notice.resolve(locale)
    );
    for source in summary.sources.iter().filter(|s| s.usage.is_some()) {
        let Some(usage) = &source.usage else {
            continue;
        };
        out += &format!("\n{}\n", cost::heading(source.source).resolve(locale));
        if let Some(note) = &source.note {
            out += &format!("  {}\n", crate::ui::safe(&note.resolve(locale)));
        }
        out += &format!(
            "  {}\n",
            usage.period.line(span.label.clone()).resolve(locale)
        );
        for model in &usage.period.models {
            out += &format!("    {}\n", crate::ui::safe(&model.line().resolve(locale)));
        }
    }
    if !summary.any_on() {
        out += &format!("\n{}\n", cost::enable_hint().resolve(locale));
    }
    out
}

/// PRD 0042 RF5.1 `--cost --by session|block|project`: the core's page as
/// JSON, or in text with at most `top` sessions or projects.
#[allow(clippy::too_many_arguments)]
fn breakdown_text(
    scanner: &Scanner,
    pricing: &PricingFile,
    by: Breakdown,
    span: Span,
    top: Option<usize>,
    json: bool,
    now: chrono::DateTime<chrono::Utc>,
    locale: &str,
) -> Result<String, String> {
    let l10n = |error: cost::CostError| error.l10n().resolve(locale);
    let zone = DayZone::Local;
    let safe =
        |text: &cuotai_core::text::Localized| crate::ui::safe(&text.resolve(locale)).into_owned();
    let header = |view: &cost::SpanView, notice: &cuotai_core::text::Localized| {
        format!(
            "{}\n{}\n",
            cost::by_heading(by, view).resolve(locale),
            notice.resolve(locale)
        )
    };
    match by {
        Breakdown::Session => {
            let page = scanner.sessions(pricing, span, now, top, 0).map_err(l10n)?;
            if json {
                return serde_json::to_string_pretty(&page)
                    .map(|text| text + "\n")
                    .map_err(|e| e.to_string());
            }
            let mut out = header(&page.span, &page.notice);
            out += &format!(
                "{}\n",
                page.total.line(page.span.label.clone()).resolve(locale)
            );
            if page.sessions.is_empty() {
                out += &format!("\n{}\n", cost::by_empty_text().resolve(locale));
            }
            for session in &page.sessions {
                out += &format!(
                    "\n{} [{}] · {}\n",
                    safe(&session.name),
                    session.short,
                    session.source.label()
                );
                out += &format!(
                    "  {}\n",
                    safe(
                        &session
                            .estimate
                            .line(cost::project_name(session.project.as_deref(), None))
                    )
                );
                for model in &session.estimate.models {
                    out += &format!("    {}\n", safe(&model.line()));
                }
            }
            let more = page.count.saturating_sub(page.sessions.len());
            if more > 0 {
                out += &format!("\n{}\n", cost::more_text(more).resolve(locale));
            }
            Ok(out)
        }
        Breakdown::Block => {
            let page = scanner
                .blocks(pricing, None, span, now, &[])
                .map_err(l10n)?;
            if json {
                return serde_json::to_string_pretty(&page)
                    .map(|text| text + "\n")
                    .map_err(|e| e.to_string());
            }
            let mut out = header(&page.span, &page.notice);
            out += &format!("{}\n", cost::local_estimate_text().resolve(locale));
            if page.blocks.is_empty() {
                out += &format!("\n{}\n", cost::by_empty_text().resolve(locale));
            }
            for block in &page.blocks {
                out += &format!("\n{}\n", safe(&cost::block_line(block, zone)));
                out += &format!(
                    "  {}\n",
                    safe(&block.estimate.line(cost::local_estimate_text()))
                );
                if let Some(window) = &block.provider_window {
                    out += &format!("  {}\n", safe(&cost::provider_window_text(window, zone)));
                }
            }
            Ok(out)
        }
        Breakdown::Project => {
            let page = scanner.projects(pricing, span, now).map_err(l10n)?;
            let shown: Vec<_> = page
                .projects
                .iter()
                .take(top.unwrap_or(usize::MAX))
                .collect();
            if json {
                let mut value = serde_json::to_value(&page).map_err(|e| e.to_string())?;
                value["projects"] = serde_json::to_value(&shown).map_err(|e| e.to_string())?;
                value["count"] = page.projects.len().into();
                return serde_json::to_string_pretty(&value)
                    .map(|text| text + "\n")
                    .map_err(|e| e.to_string());
            }
            let mut out = header(&page.span, &page.notice);
            out += &format!(
                "{}\n",
                page.total.line(page.span.label.clone()).resolve(locale)
            );
            if shown.is_empty() {
                out += &format!("\n{}\n", cost::by_empty_text().resolve(locale));
            }
            for project in &shown {
                let hidden = if project.hidden {
                    format!(" ({})", cost::hidden_text().resolve(locale))
                } else {
                    String::new()
                };
                let short = project
                    .short
                    .as_deref()
                    .map(|short| format!(" [{short}]"))
                    .unwrap_or_default();
                out += &format!("\n{}{short}{hidden}\n", safe(&project.name));
                out += &format!(
                    "  {}\n",
                    safe(&project.estimate.line(trf_in(
                        locale,
                        "cost.project.sessions",
                        "{count} session(s)",
                        &[("count", &project.sessions)],
                    )))
                );
            }
            let more = page.projects.len().saturating_sub(shown.len());
            if more > 0 {
                out += &format!("\n{}\n", cost::more_text(more).resolve(locale));
            }
            Ok(out)
        }
        Breakdown::Day | Breakdown::Model => Ok(String::new()),
    }
}

/// PRD 0042 RF5.2: the live blocks as text.
fn live_text(blocks: &[cost::LiveBlock], on: &[SourceId], locale: &str) -> String {
    let zone = DayZone::Local;
    let safe =
        |text: &cuotai_core::text::Localized| crate::ui::safe(&text.resolve(locale)).into_owned();
    let mut out = format!("{}\n", cost::estimate_notice().resolve(locale));
    if on.is_empty() {
        out += &format!("\n{}\n", cost::enable_hint().resolve(locale));
        return out;
    }
    for source in on {
        out.push('\n');
        let Some(live) = blocks.iter().find(|b| b.block.source == *source) else {
            out += &format!("{}\n", safe(&cost::live_none_text(*source)));
            continue;
        };
        out += &format!("{}\n", safe(&cost::live_window_text(live, zone)));
        out += &format!(
            "  {}\n",
            safe(&live.block.estimate.line(cost::local_estimate_text()))
        );
        match &live.line {
            Some(line) => out += &format!("  {}\n", safe(line)),
            None => out += &format!("  {}\n", safe(&cost::live_rate_missing_text())),
        }
        if let Some(projection) = &live.projection {
            out += &format!("  {}\n", safe(projection));
        }
        if let Some(window) = &live.block.provider_window {
            out += &format!("  {}\n", safe(&cost::provider_window_text(window, zone)));
        }
    }
    out
}

/// PRD 0042 RF5.2: `--cost-live` once, or every `interval` seconds (in
/// place on a terminal, `q` quits; one JSON object per line with `--json`).
fn live_loop(
    scanner: &Scanner,
    pricing: &PricingFile,
    only: Option<SourceId>,
    interval: Option<u64>,
    json: bool,
    locale: &str,
) -> Result<u8, String> {
    use crossterm::event::{self, Event, KeyCode, KeyModifiers};
    let l10n = |error: cost::CostError| error.l10n().resolve(locale);
    let io_error = |error: io::Error| error.to_string();
    let terminal = interval.is_some() && !json && io::stdout().is_terminal();
    let raw = terminal && crossterm::terminal::enable_raw_mode().is_ok();
    let result = (|| -> Result<u8, String> {
        loop {
            let now = chrono::Utc::now();
            let report = scanner.scan(now).map_err(l10n)?;
            let on: Vec<SourceId> = scanner
                .settings()
                .load()
                .map_err(l10n)?
                .enabled_sources(scanner.env())
                .into_iter()
                .filter(|s| only.is_none_or(|only| only == *s))
                .collect();
            let blocks = scanner.live(pricing, only, now, &[]).map_err(l10n)?;
            let mut stdout = io::stdout().lock();
            if json {
                let text = if interval.is_some() {
                    serde_json::to_string(&blocks)
                } else {
                    serde_json::to_string_pretty(&blocks)
                }
                .map_err(|e| e.to_string())?;
                writeln!(stdout, "{text}").map_err(io_error)?;
            } else if terminal {
                let text = live_text(&blocks, &on, locale).replace('\n', "\r\n");
                let quit = tr_in(locale, "cost.live.quit", "q quits");
                write!(stdout, "\x1b[2J\x1b[H{text}\r\n{quit}\r\n").map_err(io_error)?;
            } else {
                write!(stdout, "{}", live_text(&blocks, &on, locale)).map_err(io_error)?;
            }
            stdout.flush().map_err(io_error)?;
            drop(stdout);
            let Some(seconds) = interval else {
                return Ok(scan_code(&report));
            };
            let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
            loop {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    break;
                }
                if !raw {
                    std::thread::sleep(left);
                    break;
                }
                if event::poll(left).map_err(io_error)? {
                    if let Event::Key(key) = event::read().map_err(io_error)? {
                        let ctrl_c = key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL);
                        if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) || ctrl_c {
                            return Ok(scan_code(&report));
                        }
                    }
                }
            }
        }
    })();
    if raw {
        let _ = crossterm::terminal::disable_raw_mode();
    }
    result
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
        let (text, _) = prices(&file, false, "pt-BR").unwrap();
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

    /// PRD 0042 RF5: `--cost --by session|block|project` and `--cost-live`
    /// over a fixture: text and JSON, the `--top` cut and the hint for the
    /// rest, and the live view with its burn rate and projection.
    #[test]
    fn breakdowns_and_the_live_block_print_from_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let env = Env::new(Some(dir.path().join("home")), []);
        let settings = SettingsStore::at(dir.path().join("cost/settings.json"));
        let index = CostIndex::new(dir.path().join("cost/cost.sqlite3"));
        let disclosure = settings
            .load()
            .unwrap()
            .disclosure(SourceId::ClaudeCode, &env);
        settings
            .enable(
                SourceId::ClaudeCode,
                Vec::new(),
                &disclosure.digest,
                &env,
                chrono::Utc::now(),
            )
            .unwrap();
        let logs = dir.path().join("home/.claude/projects/p");
        std::fs::create_dir_all(&logs).unwrap();
        let now = chrono::Utc::now();
        let line = |n: u32, session: &str, cwd: &str, minutes_ago: i64| {
            let at = (now - chrono::TimeDelta::minutes(minutes_ago)).to_rfc3339();
            format!(
                r#"{{"cwd":"{cwd}","sessionId":"{session}","message":{{"id":"m{n}","model":"claude-sonnet-4-5-20250929","usage":{{"input_tokens":1000,"output_tokens":10}}}},"requestId":"r{n}","timestamp":"{at}"}}"#
            ) + "\n"
        };
        std::fs::write(
            logs.join("a.jsonl"),
            [
                line(1, "s-a", "/srv/example/one", 70),
                line(2, "s-a", "/srv/example/one", 8),
                line(3, "s-a", "/srv/example/one", 5),
                line(4, "s-a", "/srv/example/one", 2),
            ]
            .concat(),
        )
        .unwrap();
        std::fs::write(logs.join("b.jsonl"), line(5, "s-b", "/srv/example/two", 4)).unwrap();
        let scanner = Scanner::new(index, settings, env).with_limits(FOREGROUND);
        scanner.scan(now).unwrap();
        let pricing = PricingFile::at(dir.path().join("cost/pricing.json"));
        let span = Span::Days(2);
        let text = |by, top, json| {
            breakdown_text(&scanner, &pricing, by, span, top, json, now, "en-US").unwrap()
        };

        let sessions = text(Breakdown::Session, Some(1), false);
        assert!(
            sessions.starts_with("Sessions (Last 2 days)\n"),
            "{sessions}"
        );
        assert!(sessions.contains("Session · "), "{sessions}");
        assert!(
            sessions.contains("1 more; --all lists every one."),
            "{sessions}"
        );
        assert!(!sessions.contains("/srv/example"), "{sessions}");
        let all: serde_json::Value =
            serde_json::from_str(&text(Breakdown::Session, None, true)).unwrap();
        assert_eq!(all["count"], 2);
        assert_eq!(all["sessions"].as_array().unwrap().len(), 2);

        let projects = text(Breakdown::Project, None, false);
        assert!(
            projects.starts_with("Projects (Last 2 days)\n"),
            "{projects}"
        );
        assert_eq!(projects.matches("Project ").count(), 2, "{projects}");
        assert!(projects.contains("session(s)"), "{projects}");

        let blocks = text(Breakdown::Block, None, false);
        assert!(blocks.contains("Local estimate"), "{blocks}");
        assert!(blocks.contains("· active ·"), "{blocks}");
        let page: serde_json::Value =
            serde_json::from_str(&text(Breakdown::Block, None, true)).unwrap();
        assert_eq!(page["blocks"][0]["active"], true);

        let live = scanner.live(&pricing, None, now, &[]).unwrap();
        let shown = live_text(&live, &[SourceId::ClaudeCode], "en-US");
        assert!(shown.contains("local estimate"), "{shown}");
        assert!(shown.contains("tok/min"), "{shown}");
        let none = live_text(&[], &[SourceId::CodexCli], "en-US");
        assert!(none.contains("Codex CLI: no active block."), "{none}");
        let off = live_text(&[], &[], "en-US");
        assert!(off.contains("cuotai --cost-enable"), "{off}");
    }
}
