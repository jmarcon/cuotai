mod auth_ui;
mod dashboard;
mod ui;
mod wizard;

use clap::Parser;
use cuotai_core::{
    config_file::ConfigFile, oauth::OAuthSession, EnvironmentCredentials, Monitor, Provider,
    Snapshot, Status,
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
    ui::run(file, session).await?;
    Ok(0)
}

fn print_snapshot(snapshot: &Snapshot) {
    println!("CUOTAI • {}", snapshot.finished_at.to_rfc3339());
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
