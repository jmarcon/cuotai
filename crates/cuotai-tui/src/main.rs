mod about_view;
mod auth_ui;
mod dashboard;
mod models_view;
mod motion;
mod ui;
mod wizard;

use chrono::{DateTime, Utc};
use clap::Parser;
use cuotai_core::{
    config_file::ConfigFile, metrics, oauth::OAuthSession, EnvironmentCredentials, Monitor,
    Provider, Snapshot, Status,
};
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    sync::Arc,
};

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
    /// Consulta uma vez e emite o Snapshot em JSON (sem credenciais).
    #[arg(long)]
    json: bool,
    /// Lista os identificadores aceitos na configuração.
    #[arg(long)]
    list_providers: bool,
    /// SOMENTE desenvolvimento: tokens sem criptografia em SQLite (release recusa).
    #[arg(long, value_name = "ARQUIVO")]
    dev_credentials_sqlite: Option<PathBuf>,
    /// Desliga as animações da TUI (também: CUOTAI_NO_ANIMATIONS=1).
    #[arg(long)]
    no_animations: bool,
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
    let interactive = !args.once && !args.json;
    if interactive && (!io::stdout().is_terminal() || !io::stdin().is_terminal()) {
        return Err("A TUI requer terminal interativo. Use --once ou --json.".into());
    }
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
        let monitor = Monitor::with_oauth(file.config, Arc::new(EnvironmentCredentials), session)?;
        let snapshot = monitor.refresh().await;
        if args.json {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer_pretty(&mut stdout, &snapshot)?;
            writeln!(stdout)?;
        } else {
            print_snapshot(&snapshot);
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
    ui::run(file, session, animations).await?;
    Ok(0)
}

fn print_snapshot(snapshot: &Snapshot) {
    println!("CUOTAI • {}", snapshot.finished_at.to_rfc3339());
    print!(
        "\n{}",
        summary_lines(snapshot, ui::tui_locale(), ui::local_time)
    );
    for report in &snapshot.reports {
        println!(
            "\n{} / {} — {}",
            ui::safe(&report.account),
            report.provider.label(),
            report.status.label()
        );
        for metric in &report.metrics {
            println!("  {}: {}", ui::safe(&metric.label), ui::metric_text(metric));
            if let Some(period) = &metric.period {
                println!("    Período: {}", ui::safe(period));
            }
            if let Some(reset) = metric.resets_at {
                println!("    Renova: {}", reset.to_rfc3339());
            }
        }
        for (idx, fallback_text) in report.notes.iter().enumerate() {
            let text = report
                .notes_l10n
                .get(idx)
                .map(ui::resolve_l10n)
                .unwrap_or_else(|| fallback_text.clone());
            println!("  {}", ui::safe(&text));
        }
        if !report.source.is_empty() {
            println!("  Fonte: {}", ui::safe(&report.source));
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use cuotai_core::{Metric, MetricKind, MetricWindow, Report};

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
}
