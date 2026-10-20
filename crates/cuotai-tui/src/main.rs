mod about_view;
mod auth_ui;
mod dashboard;
mod models_view;
mod motion;
mod ui;
mod wizard;

use chrono::{DateTime, TimeDelta, Utc};
use clap::Parser;
use cuotai_core::{
    config_file::ConfigFile,
    history::{HistoryStore, MetricHistory, Retention},
    metrics,
    oauth::OAuthSession,
    pace::{self, Point},
    Account, Config, EnvironmentCredentials, Monitor, Provider, Report, Snapshot, Status,
};
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

/// How long `--once`/`--json` wait for the readings to reach the history.
const HISTORY_FLUSH: Duration = Duration::from_secs(5);
/// PRD 0006 RF6.2: days `--history` prints without `--history-days`.
const HISTORY_DEFAULT_DAYS: u32 = 7;
/// Sparkline cells of each `--history` metric.
const HISTORY_SPARK_CELLS: usize = 60;

#[derive(Parser)]
#[command(
    version,
    about = "Quota e consumo de AI: APIs e assinaturas, sem gerar tokens."
)]
struct Args {
    /// Arquivo TOML (cria se ausente). Sem ele: CUOTAI_CONFIG, ./cuotai.toml existente,
    /// ou <config do SO>/cuotai/config.toml (onde o wizard cria um novo).
    #[arg(short, long)]
    config: Option<PathBuf>,
    /// Consulta uma vez, sem abrir a TUI.
    #[arg(long)]
    once: bool,
    /// Consulta uma vez e emite o Snapshot em JSON (sem credenciais); com
    /// --history, emite o histórico em JSON.
    #[arg(long)]
    json: bool,
    /// Mostra o histórico local de medições de CONTA (ou de todas) e sai,
    /// sem consultar os provedores.
    #[arg(long, value_name = "CONTA", num_args = 0..=1, conflicts_with = "once")]
    history: Option<Option<String>>,
    /// Lista os identificadores aceitos na configuração.
    #[arg(long)]
    list_providers: bool,
    /// SOMENTE desenvolvimento: tokens sem criptografia em SQLite (release recusa).
    #[arg(long, value_name = "ARQUIVO")]
    dev_credentials_sqlite: Option<PathBuf>,
    /// Desliga as animações da TUI (também: CUOTAI_NO_ANIMATIONS=1).
    #[arg(long)]
    no_animations: bool,
    /// Dias de histórico local de medições mantidos (7 a 365; padrão 90);
    /// com --history, os dias mostrados (padrão 7).
    #[arg(long, value_name = "DIAS")]
    history_days: Option<u32>,
}

/// PRD 0006 RF4.1: the retention of `--history-days`, or the default.
fn retention(days: Option<u32>) -> Result<Retention, String> {
    days.map_or(Ok(Retention::default()), |days| {
        Retention::days(days).map_err(|error| error.l10n().resolve(ui::tui_locale()))
    })
}

/// PRD 0006: the history of `file` in the OS data dir, pruned by
/// `retention`; `None` (nothing recorded) without a resolvable data dir.
pub(crate) fn history_store(file: &ConfigFile, retention: Retention) -> Option<HistoryStore> {
    HistoryStore::in_data_dir(file)
        .ok()
        .map(|store| store.with_retention(retention))
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    match run().await {
        Ok(code) => std::process::ExitCode::from(code),
        Err(error) => {
            eprintln!("cuotai: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<u8, Box<dyn std::error::Error>> {
    let args = Args::parse();
    if args.list_providers {
        for provider in Provider::ALL {
            println!(
                "{:<20} {}",
                serde_json::to_value(provider)?.as_str().unwrap_or_default(),
                provider.label()
            );
        }
        return Ok(0);
    }
    if let Some(account) = &args.history {
        let days = retention(Some(args.history_days.unwrap_or(HISTORY_DEFAULT_DAYS)))?.keep_days();
        let file = ConfigFile::load(args.config, false)?;
        let store = HistoryStore::in_data_dir(&file)
            .map_err(|error| error.l10n().resolve(ui::tui_locale()))?;
        let now = Utc::now();
        let since = now - TimeDelta::days(i64::from(days));
        let found = account_histories(&store, &file.config, account.as_deref(), since, now)?;
        let mut stdout = io::stdout().lock();
        if args.json {
            serde_json::to_writer_pretty(&mut stdout, &history_json(&found, since, now, days))?;
            writeln!(stdout)?;
        } else {
            write!(
                stdout,
                "{}",
                history_text(&found, since, now, days, ui::local_time)
            )?;
        }
        return Ok(history_exit_code(&found));
    }
    let interactive = !args.once && !args.json;
    if interactive && (!io::stdout().is_terminal() || !io::stdin().is_terminal()) {
        return Err("A TUI requer terminal interativo. Use --once ou --json.".into());
    }
    let retention = retention(args.history_days)?;
    let file = ConfigFile::load(args.config, interactive)?;
    let session = Arc::new(match args.dev_credentials_sqlite {
        Some(path) => {
            let session = OAuthSession::development_sqlite(&path)?;
            eprintln!("AVISO: credenciais em SQLite SEM CRIPTOGRAFIA (desenvolvimento); Keychain não será usado. Não compartilhe o banco.");
            session
        }
        None => OAuthSession::system()?,
    });
    if args.once || args.json {
        let history = history_store(&file, retention);
        let mut monitor =
            Monitor::with_oauth(file.config, Arc::new(EnvironmentCredentials), session)?;
        if let Some(store) = history {
            monitor = monitor.with_history(store);
        }
        monitor.mark_history_removed();
        let snapshot = monitor.refresh().await;
        // The readings are queued off the refresh; let them land first.
        monitor.flush_history(HISTORY_FLUSH);
        if args.json {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer_pretty(&mut stdout, &snapshot)?;
            writeln!(stdout)?;
        } else {
            let now = Utc::now();
            let recent: Vec<Vec<Vec<Point>>> = snapshot
                .reports
                .iter()
                .enumerate()
                .map(|(index, report)| monitor.recent_points(index, &report.metrics, now))
                .collect();
            print_snapshot(&snapshot, &recent, now);
        }
        if let Some(warning) = monitor.history_status().and_then(|s| s.warning()) {
            eprintln!("cuotai: {}", warning.resolve(ui::tui_locale()));
        }
        return Ok(
            if snapshot.reports.iter().all(|r| r.status == Status::Ready) {
                0
            } else {
                2
            },
        );
    }
    let animations = motion::enabled(args.no_animations, |key| std::env::var(key).ok());
    ui::run(file, session, animations, retention).await?;
    Ok(0)
}

fn print_snapshot(snapshot: &Snapshot, recent: &[Vec<Vec<Point>>], now: DateTime<Utc>) {
    println!("CUOTAI • {}", snapshot.finished_at.to_rfc3339());
    print!(
        "\n{}",
        summary_lines(snapshot, ui::tui_locale(), ui::local_time)
    );
    for (index, report) in snapshot.reports.iter().enumerate() {
        let recent = recent.get(index).map_or(&[][..], Vec::as_slice);
        print!(
            "{}",
            report_text(report, recent, now, ui::tui_locale(), ui::local_time)
        );
    }
}

/// One `--once` account block: heading, metrics (with the PRD 0007 RF4 pace
/// line under each eligible one, projected from the recorded `recent`
/// points of each metric when there are enough), notes and source.
fn report_text(
    report: &Report,
    recent: &[Vec<Point>],
    now: DateTime<Utc>,
    locale: &str,
    at: impl Fn(DateTime<Utc>) -> String,
) -> String {
    let mut out = format!("\n{}\n", report_heading(report, now));
    for (index, metric) in report.metrics.iter().enumerate() {
        out += &format!(
            "  {}: {}\n",
            ui::safe(&metric.label),
            ui::metric_text(metric)
        );
        if let Some(period) = &metric.period {
            out += &format!("    Período: {}\n", ui::safe(period));
        }
        if let Some(reset) = metric.resets_at {
            out += &format!("    Renova: {}\n", reset.to_rfc3339());
        }
        let points = recent.get(index).map(Vec::as_slice);
        if let Some(pace) = pace::assess(metric, now, points) {
            let text = dashboard::pace_text(&pace, locale, &at);
            out += &format!("    Ritmo: {text} (estimativa linear)\n");
        }
    }
    for (idx, fallback_text) in report.notes.iter().enumerate() {
        let text = report
            .notes_l10n
            .get(idx)
            .map(|note| note.resolve(locale))
            .unwrap_or_else(|| fallback_text.clone());
        out += &format!("  {}\n", ui::safe(&text));
    }
    if !report.source.is_empty() {
        out += &format!("  Fonte: {}\n", ui::safe(&report.source));
    }
    out
}

/// PRD 0009 RF3.2: one line per account with the core's binding summary;
/// accounts without one show their status (or class when ready).
fn summary_lines(
    snapshot: &Snapshot,
    locale: &str,
    at: impl Fn(DateTime<Utc>) -> String,
) -> String {
    let mut out = String::new();
    for report in &snapshot.reports {
        let state = match metrics::binding(report) {
            Some(limit) => limit.summary(&at).resolve(locale),
            None if report.status == Status::Ready => {
                dashboard::class_label(dashboard::classification(Some(report))).to_string()
            }
            None => report.status.label().to_string(),
        };
        out.push_str(&format!(
            "{}  {}\n",
            ui::safe(&report.account),
            ui::safe(&state)
        ));
    }
    out
}

/// PRD 0010 RF5.2: `--once` account heading with the measurement age.
fn report_heading(report: &Report, now: DateTime<Utc>) -> String {
    format!(
        "{} / {} — {} · consultado {}",
        ui::safe(&report.account),
        report.provider.label(),
        report.status.label(),
        ui::age_text(report.checked_at, now)
    )
}

/// PRD 0006 RF6.2: the recorded metrics of each account named `name` (all
/// accounts without one) within `since..=now`, in configuration order. An
/// unknown name is a local error; a storage failure is never "no data".
fn account_histories<'a>(
    store: &HistoryStore,
    config: &'a Config,
    name: Option<&str>,
    since: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<Vec<(&'a Account, Vec<MetricHistory>)>, String> {
    let accounts: Vec<&Account> = config
        .accounts
        .iter()
        .filter(|account| name.is_none_or(|name| account.name == name))
        .collect();
    if let (Some(name), true) = (name, accounts.is_empty()) {
        return Err(format!(
            "Conta não encontrada na configuração: {}",
            ui::safe(name)
        ));
    }
    accounts
        .into_iter()
        .map(|account| {
            store
                .account_history(account, since..=now)
                .map(|metrics| (account, metrics))
                .map_err(|error| error.l10n().resolve(ui::tui_locale()))
        })
        .collect()
}

/// `0` when every listed account has recorded data in the span, `2` when
/// one (or the configuration) has none, like the other single-run outputs.
fn history_exit_code(found: &[(&Account, Vec<MetricHistory>)]) -> u8 {
    if !found.is_empty() && found.iter().all(|(_, metrics)| !metrics.is_empty()) {
        0
    } else {
        2
    }
}

/// `--history --json`: the span and, per account, the core's metrics
/// (points and window instances); no credentials, only the account name
/// and provider the TOML already holds.
fn history_json(
    found: &[(&Account, Vec<MetricHistory>)],
    since: DateTime<Utc>,
    now: DateTime<Utc>,
    days: u16,
) -> serde_json::Value {
    serde_json::json!({
        "from": since,
        "to": now,
        "days": days,
        "accounts": found
            .iter()
            .map(|(account, metrics)| serde_json::json!({
                "account": account.name,
                "provider": account.provider,
                "metrics": metrics,
            }))
            .collect::<Vec<_>>(),
    })
}

/// `--history` text: per account, each metric's summary, sparkline over
/// the span and window instances; an account without samples says so.
fn history_text(
    found: &[(&Account, Vec<MetricHistory>)],
    since: DateTime<Utc>,
    now: DateTime<Utc>,
    days: u16,
    at: impl Fn(DateTime<Utc>) -> String,
) -> String {
    let mut out = format!(
        "CUOTAI • histórico de {days} dias até {}\n",
        now.to_rfc3339()
    );
    if found.is_empty() {
        out += "\nNenhuma conta na configuração; sem medições para mostrar.\n";
    }
    for (account, metrics) in found {
        out += &format!(
            "\n{} / {}\n",
            ui::safe(&account.name),
            account.provider.label()
        );
        if metrics.is_empty() {
            out += &format!("  Sem medições registradas nos últimos {days} dias.\n");
        }
        for metric in metrics {
            let Some(trend) = metric.trend() else {
                out += &format!(
                    "  {}: sem valores medidos no período\n",
                    ui::safe(&metric.label)
                );
                continue;
            };
            out += &format!(
                "  {}: {} medições · {} ({})\n",
                ui::safe(&metric.label),
                metric.points.len(),
                dashboard::trend_text(&trend, &metric.unit),
                at(trend.last.taken_at)
            );
            out += &format!(
                "    {}\n",
                dashboard::sparkline(metric, since, now, HISTORY_SPARK_CELLS).trim_end()
            );
            for window in &metric.windows {
                let resets = window
                    .resets_at
                    .map(|reset| format!(" · renova {}", at(reset)))
                    .unwrap_or_default();
                out += &format!(
                    "    Janela {} → {}: {} → {}, pico {} em {}{resets}\n",
                    at(window.started_at),
                    at(window.ended_at),
                    dashboard::compact(window.initial_used),
                    dashboard::compact(window.final_used),
                    dashboard::compact(window.peak_used),
                    at(window.peak_at),
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use cuotai_core::{Metric, MetricKind, MetricWindow};

    fn quota(window: MetricWindow, used: f64, resets_at: Option<DateTime<Utc>>) -> Metric {
        Metric {
            label: format!("{window:?}"),
            used: Some(used),
            limit: Some(100.0),
            remaining: None,
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(window),
            period: None,
            resets_at,
        }
    }

    fn report(account: &str, status: Status, metrics: Vec<Metric>) -> Report {
        Report {
            account: account.into(),
            provider: Provider::Claude,
            checked_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            status,
            metrics,
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
        }
    }

    #[test]
    fn once_summary_shows_binding_limit_per_account() {
        let week_reset = Utc.with_ymd_and_hms(2026, 1, 3, 14, 30, 0).unwrap();
        let snapshot = Snapshot {
            started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            finished_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
            reports: vec![
                // 5 h free, week exhausted: blocked by the week.
                report(
                    "Claude",
                    Status::Ready,
                    vec![
                        quota(MetricWindow::Window5h, 0.0, None),
                        quota(MetricWindow::Weekly, 100.0, Some(week_reset)),
                    ],
                ),
                report(
                    "Work",
                    Status::Ready,
                    vec![
                        quota(MetricWindow::Window5h, 10.0, None),
                        quota(MetricWindow::Weekly, 62.0, None),
                    ],
                ),
                // Exhausted without a known return: never "now".
                report(
                    "Night",
                    Status::Ready,
                    vec![quota(MetricWindow::Monthly, 100.0, None)],
                ),
                report("Ops", Status::AuthenticationError, Vec::new()),
                report("Empty", Status::Ready, Vec::new()),
            ],
        };
        let at = |t: DateTime<Utc>| t.format("%H:%M").to_string();
        assert_eq!(
            summary_lines(&snapshot, "en-US", at),
            "Claude  blocked until 14:30 by the weekly window\n\
             Work  most restrictive: weekly window at 62%\n\
             Night  blocked by the monthly window; return time unknown\n\
             Ops  Autenticação\n\
             Empty  Sem dados\n"
        );
        assert_eq!(
            summary_lines(&snapshot, "pt-BR", at).lines().next(),
            Some("Claude  bloqueada até 14:30 pela janela semanal")
        );
    }

    #[test]
    fn once_heading_carries_the_measurement_age() {
        let report = report("Work", Status::RateLimited, Vec::new());
        let now = report.checked_at + chrono::Duration::seconds(125);
        assert_eq!(
            report_heading(&report, now),
            "Work / Claude — Rate limit · consultado há 2 min"
        );
    }

    #[test]
    fn once_prints_the_pace_line_under_each_eligible_metric() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let resets = |minutes| Some(now + chrono::Duration::minutes(minutes));
        let report = report(
            "Work",
            Status::Ready,
            vec![
                quota(MetricWindow::Window5h, 40.0, resets(120)),
                quota(MetricWindow::Weekly, 88.0, resets(30 * 60)),
                // Freshly opened, then no reset: no invented pace.
                quota(MetricWindow::Daily, 1.0, resets(24 * 60 - 5)),
                quota(MetricWindow::Monthly, 10.0, None),
            ],
        );
        let at = |t: DateTime<Utc>| t.format("%H:%M").to_string();
        assert_eq!(
            report_text(&report, &[], now, "en-US", at),
            "\nWork / Claude — Atualizado · consultado agora\n\
             \x20 Window5h: 40.00 % / 100.00\n\
             \x20   Renova: 2026-01-01T02:00:00+00:00\n\
             \x20   Ritmo: 20 points in reserve · lasts until the reset (estimativa linear)\n\
             \x20 Weekly: 88.00 % / 100.00\n\
             \x20   Renova: 2026-01-02T06:00:00+00:00\n\
             \x20   Ritmo: 6 points above the pace · runs out at 18:49, 11h 10m before the reset (estimativa linear)\n\
             \x20 Daily: 1.00 % / 100.00\n\
             \x20   Renova: 2026-01-01T23:55:00+00:00\n\
             \x20 Monthly: 10.00 % / 100.00\n"
        );
        assert!(report_text(&report, &[], now, "es", at)
            .contains("Ritmo: 6 puntos por encima del ritmo · se agota a las 18:49"));

        // PRD 0007 RF1.5 with the recorded history: the 5-hour window rose
        // 30 points in the last 40 minutes, so it runs out before the reset.
        let point = |minutes: i64, used: f64| Point {
            taken_at: now - chrono::Duration::minutes(minutes),
            used,
        };
        let recent = [vec![point(40, 10.0), point(20, 25.0), point(0, 40.0)]];
        assert!(report_text(&report, &recent, now, "en-US", at)
            .contains("Ritmo: 20 points in reserve · runs out at 01:20, 40m 00s before the reset"));
    }

    #[test]
    fn history_mode_prints_the_span_and_exits_like_the_other_single_runs() {
        use cuotai_core::config_file::ConfigIdentity;
        let dir = std::env::temp_dir().join(format!("cuotai-tui-history-{}", std::process::id()));
        let path = dir.join("history.sqlite3");
        let store = HistoryStore::new(path.clone(), &ConfigIdentity::Explicit("cli".into()));
        let config = Config::from_toml(
            "[[accounts]]\nname = \"Work\"\nprovider = \"groq\"\ncredential_env = \"GROQ_WORK\"\n\
             [[accounts]]\nname = \"Spare\"\nprovider = \"groq\"\ncredential_env = \"GROQ_SPARE\"\n",
        )
        .unwrap();
        let now = Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap();
        let since = now - TimeDelta::days(7);
        let at = |t: DateTime<Utc>| t.format("%m-%d %H:%M").to_string();

        // Nothing recorded: it says so (exit 2) and never creates the file.
        let found = account_histories(&store, &config, None, since, now).unwrap();
        assert_eq!(history_exit_code(&found), 2);
        let text = history_text(&found, since, now, 7, at);
        assert_eq!(
            text.matches("Sem medições registradas nos últimos 7 dias.")
                .count(),
            2
        );
        assert!(!path.exists());
        assert!(account_histories(&store, &config, Some("Nope"), since, now)
            .unwrap_err()
            .contains("Conta não encontrada"));

        // Two readings of one 5-hour window, a reset, and an old sample
        // outside the span.
        let hours = |n: i64| now + TimeDelta::hours(n);
        let work = &config.accounts[0];
        for (taken, used, reset) in [
            (-200, 99.0, -190),
            (-3, 10.0, 2),
            (-2, 70.0, 2),
            (-1, 5.0, 4),
        ] {
            let metric = quota(MetricWindow::Window5h, used, Some(hours(reset)));
            store.record(work, hours(taken), &[metric]).unwrap();
        }
        let found = account_histories(&store, &config, Some("Work"), since, now).unwrap();
        assert_eq!(history_exit_code(&found), 0);
        assert_eq!(
            history_text(&found, since, now, 7, at),
            format!(
                "CUOTAI • histórico de 7 dias até 2026-01-02T00:00:00+00:00\n\
                 \nWork / {}\n\
                 \x20 Window5h: 3 medições · mín 5 · máx 70 · último 5 % (01-01 23:00)\n\
                 \x20   {}▂▆\n\
                 \x20   Janela 01-01 21:00 → 01-01 22:00: 10 → 70, pico 70 em 01-01 22:00 · renova 01-02 02:00\n\
                 \x20   Janela 01-01 23:00 → 01-01 23:00: 5 → 5, pico 5 em 01-01 23:00 · renova 01-02 04:00\n",
                Provider::Groq.label(),
                " ".repeat(58)
            )
        );
        let json = history_json(&found, since, now, 7);
        let metric = &json["accounts"][0]["metrics"][0];
        assert_eq!(json["accounts"][0]["account"], "Work");
        assert_eq!(metric["points"].as_array().unwrap().len(), 3);
        assert_eq!(metric["windows"][0]["peak_used"], 70.0);
        assert_eq!(metric["limit"], 100.0);
        // Every account: one without data makes it partial.
        let found = account_histories(&store, &config, None, since, now).unwrap();
        assert_eq!(history_exit_code(&found), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
