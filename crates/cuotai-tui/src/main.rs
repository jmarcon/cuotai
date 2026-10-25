mod about_view;
mod auth_ui;
mod canary;
mod changelog_view;
mod cost;
mod dashboard;
mod extension_check;
mod extensions_view;
mod headless;
mod models_view;
mod motion;
mod notify;
mod report_problem;
mod report_view;
mod serve;
mod ui;
mod watch;
mod wizard;

use chrono::{DateTime, TimeDelta, Utc};
use clap::Parser;
use cuotai_core::{
    config_file::{ConfigFile, ConfigIdentity},
    export::v1::{Export, Options},
    history::{HistoryStore, MetricHistory, Retention},
    incidents::ProviderStatus,
    install_source::detect_install_source,
    metrics,
    oauth::OAuthSession,
    pace::{self, Point},
    refresh::RefreshCycle,
    Account, Config, EnvironmentCredentials, Monitor, Provider, Report, Snapshot, Status,
};
use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
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
    about = "Quota e consumo de AI: APIs e assinaturas, sem gerar tokens.",
    // PRD 0015: one local cost action at a time, never with a query mode.
    group(
        clap::ArgGroup::new("cost_action")
            .multiple(false)
            .conflicts_with_all(["once", "json_v1", "history", "notify", "serve", "watch"])
    ),
    // PRD 0031 RF10.2: `--strict-exit` belongs to the foreground modes.
    group(
        clap::ArgGroup::new("headless")
            .args(["serve", "watch"])
            .multiple(true)
    )
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
    /// Consulta CONTA uma vez e imprime o relatório de problema (texto em
    /// inglês, sem segredos, e a URL do GitHub já preenchida). Nada é
    /// enviado: você abre a URL. Saída 0; 1 se a conta não existe ou em erro
    /// local.
    #[arg(
        long,
        value_name = "CONTA",
        conflicts_with_all = [
            "list_providers", "changelog", "extension_check", "once", "json",
            "json_v1", "history", "notify", "serve", "watch", "install_source",
            "cost_action",
        ]
    )]
    report_problem: Option<String>,
    /// PRD 0028 RF4: o canário de contrato local. Consulta uma vez as contas
    /// configuradas (as cobradas por consulta ficam de fora), grava um
    /// registro sem valores em `<dados>/cuotai/canary/`, compara com a
    /// execução anterior e imprime o relatório em Markdown e um rascunho de
    /// issue por desvio. Nada é aberto nem enviado sozinho. Saída 0 sem
    /// desvio, 2 com desvio, 1 em erro local.
    #[arg(
        long,
        conflicts_with_all = [
            "list_providers", "changelog", "extension_check", "once", "json",
            "json_v1", "history", "notify", "serve", "watch", "install_source",
            "cost_action", "report_problem",
        ]
    )]
    canary: bool,
    /// Com --canary, o diretório dos registros (padrão: `<dados>/cuotai/canary`).
    #[arg(long, value_name = "DIR", requires = "canary")]
    out: Option<std::path::PathBuf>,
    /// Com --canary, calcula e imprime sem gravar nada.
    #[arg(long, requires = "canary")]
    dry_run: bool,
    /// Com --canary, abre no navegador o link de cada rascunho de issue.
    #[arg(long, requires = "canary")]
    canary_open: bool,
    /// Com --canary, inclui também as contas cobradas por consulta.
    #[arg(long, requires = "canary")]
    canary_include_metered: bool,
    /// Informa de onde esta cópia foi instalada, por que o atualizador
    /// interno deve ficar desligado e o comando de atualização, quando
    /// conhecido. É local: não lê a configuração nem consulta a rede.
    /// Saída 0; 1 em erro local.
    #[arg(
        long,
        conflicts_with_all = [
            "list_providers",
            "changelog",
            "extension_check",
            "once",
            "json",
            "json_v1",
            "history",
            "notify",
            "serve",
            "watch",
            "cost_action",
        ]
    )]
    install_source: bool,
    /// Mostra o histórico de versões do CHANGELOG embutido (sem rede e sem
    /// consultar provedores): tudo, ou só a seção de VERSÃO (uma versão
    /// como 0.68.2, ou uma série como 0.68.x). Uma versão interna nunca
    /// publicada (0.1.0 a 0.79.0) mostra o histórico de pré-lançamento
    /// que a resume, com um aviso. Os rótulos de categoria
    /// seguem o idioma da interface; as entradas continuam em inglês, o
    /// idioma do changelog, como o próprio comando avisa. Não combina com
    /// os modos de consulta, com --list-providers nem com as opções de
    /// custo. Saída 0; 1 em erro local (versão inexistente).
    #[arg(
        long,
        value_name = "VERSÃO",
        num_args = 0..=1,
        conflicts_with_all = [
            "list_providers",
            "once",
            "json",
            "json_v1",
            "history",
            "notify",
            "serve",
            "watch",
            "extension_check",
            "cost_action",
        ]
    )]
    changelog: Option<Option<String>>,
    /// Executa a extensão ID uma vez e mostra o relatório normalizado ou as
    /// violações do contrato (PRD 0017 RF2.4), sem nunca mostrar a saída
    /// bruta. Uma extensão nasce desativada: na primeira vez, e após
    /// qualquer mudança no manifesto ou no programa, o comando exato, a
    /// pasta, as variáveis de ambiente pedidas e o intervalo são listados e
    /// pedem confirmação no terminal (sem terminal: erro). Com --json, em
    /// JSON. Saída 0 se a leitura está pronta; 2 se a execução falhou ou a
    /// saída violou o contrato; 1 em erro local (id desconhecido, aprovação
    /// recusada). Não combina com --once, --json-v1, --history, --notify,
    /// --serve, --watch, --list-providers nem as opções de custo.
    #[arg(
        long,
        value_name = "ID",
        conflicts_with_all = [
            "list_providers",
            "once",
            "json_v1",
            "history",
            "notify",
            "serve",
            "watch",
            "cost_action",
        ]
    )]
    extension_check: Option<String>,
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
    /// Com a TUI aberta, avisa quando uma cota usada renova e quando uma cota
    /// com limite atinge 75% ou 90% de uso, esgota ou volta a ter folga: faixa
    /// no cabeçalho, sino do terminal (BEL) e notificação OSC 9 no iTerm2,
    /// WezTerm e Ghostty.
    #[arg(long, conflicts_with_all = ["once", "json", "history"])]
    notify: bool,
    /// Com --notify, avisa também quando, no ritmo recente, uma cota vai
    /// acabar antes da renovação: 15, 30 ou 60 minutos antes (60 sem
    /// valor). Estimativa linear das leituras recentes; desligado sem esta
    /// opção. Só nesta sessão.
    #[arg(
        long,
        value_name = "MINUTOS",
        num_args = 0..=1,
        default_missing_value = "60",
        value_parser = ["15", "30", "60"],
        requires = "notify"
    )]
    notify_projected: Option<String>,
    /// Consulta as páginas públicas de status dos provedores configurados
    /// (sem credenciais, cookies ou dados da conta; cada página no máximo a
    /// cada 5 min) e mostra incidentes nos cartões, nos detalhes e no
    /// --once. Desligado por padrão; também: CUOTAI_STATUS=1.
    #[arg(long)]
    status: bool,
    /// Não consulta as páginas de status, mesmo com --status ou CUOTAI_STATUS.
    #[arg(long)]
    no_status: bool,
    /// Consulta uma vez e emite o export v1 em JSON: o contrato versionado
    /// (docs/export-v1.md) com classe, idade, leitura preservada, limite,
    /// ritmo e acesso por conta, sem credenciais.
    #[arg(long, conflicts_with_all = ["once", "json", "history", "notify"])]
    json_v1: bool,
    /// Servidor HTTP local somente leitura em 127.0.0.1, sem abrir a TUI e
    /// sem exigir terminal: mantém o ciclo de atualização e serve o export
    /// v1 (/health, /v1/snapshot, /v1/accounts/{chave}, /v1/limits) sem
    /// consultar provedores por requisição. SIGINT/SIGTERM encerram, com
    /// saída 0 (com --strict-exit: 2 se nenhuma conta ficou saudável até o
    /// encerramento); 1 em erro local (porta ocupada, configuração, token).
    #[arg(long, conflicts_with_all = ["once", "json", "json_v1", "history", "notify"])]
    serve: bool,
    /// Porta do --serve, e só ela (ocupada: erro, saída 1). Sem --port: 6738,
    /// também sem recuo para outra porta.
    #[arg(long, value_name = "PORTA", requires = "serve")]
    port: Option<u16>,
    /// Arquivo com o token que o --serve exige em Authorization: Bearer (16
    /// a 512 caracteres, sem espaços). Sem ele, o --serve cria (na primeira
    /// vez) e exige o token guardado no diretório de dados, só legível pelo
    /// seu usuário, e diz onde ele está.
    #[arg(
        long,
        value_name = "ARQUIVO",
        requires = "serve",
        conflicts_with = "no_token"
    )]
    token_file: Option<PathBuf>,
    /// Serve sem token: qualquer usuário ou programa deste computador pode
    /// ler o seu uso (páginas web e outros computadores seguem recusados).
    #[arg(long, requires = "serve")]
    no_token: bool,
    /// Acompanha as contas sem abrir a TUI e sem exigir terminal: mantém o
    /// ciclo de atualização e emite um evento por transição real (cota
    /// cruzou um limiar, esgotou, renovou, conta bloqueou, consulta falhou
    /// ou voltou, autenticação necessária, status do provedor, uso
    /// atualizado no máximo a cada 10 min). A primeira leitura de cada
    /// conta é só a base. Combina com --serve no mesmo ciclo.
    /// SIGINT/SIGTERM/SIGHUP encerram, com saída 0 (com --strict-exit: 2 se
    /// alguma conta terminou em falha); 1 em erro local.
    #[arg(long, conflicts_with_all = ["once", "json", "json_v1", "history", "notify"])]
    watch: bool,
    /// Intervalo do --watch em segundos (60 a 86400; menor é erro, nunca
    /// ajustado): substitui o refresh_seconds global nesta execução; contas
    /// com refresh_seconds próprio o mantêm e AWS Bedrock nunca abaixo de
    /// 300.
    #[arg(long, value_name = "SEGUNDOS", requires = "watch")]
    interval: Option<u64>,
    /// Só estes eventos no --watch, separados por vírgula (padrão: todos),
    /// p. ex. quota_threshold_crossed,refresh_failed.
    #[arg(long, value_name = "LISTA", value_delimiter = ',', requires = "watch")]
    events: Option<Vec<String>>,
    /// Com --serve ou --watch, devolve a saída 2 ao encerrar por sinal quando
    /// alguma conta terminou em falha (--watch) ou nenhuma ficou saudável
    /// (--serve), para supervisores que querem isso; sem a opção, o
    /// encerramento por SIGINT/SIGTERM sai com 0.
    #[arg(long, requires = "headless")]
    strict_exit: bool,
    /// Antecedência, em minutos, do evento quota_exhaustion_projected do
    /// --watch: 15, 30 ou 60 (padrão 60).
    #[arg(
        long,
        value_name = "MINUTOS",
        value_parser = ["15", "30", "60"],
        requires = "watch"
    )]
    projected_lead: Option<String>,
    /// Com --watch, um evento JSON por linha (NDJSON) no stdout;
    /// mensagens e diagnósticos vão ao stderr.
    #[arg(long, requires = "watch")]
    jsonl: bool,
    /// Com --watch, executa as regras de hooks.toml (no diretório do
    /// arquivo TOML) a cada evento: o comando é uma lista de argumentos,
    /// sem shell; o evento chega em JSON no stdin e em CUOTAI_EVENT,
    /// CUOTAI_ACCOUNT, CUOTAI_PROVIDER e CUOTAI_METRIC; o ambiente se
    /// reduz a PATH, HOME e LANG. Na primeira vez, e após qualquer mudança
    /// nas regras, lista os comandos e pede confirmação no terminal (sem
    /// terminal: erro). Sem --jsonl, o stdout fica vazio.
    #[arg(long, requires = "watch")]
    hooks: bool,
    /// Lista as fontes de custo local (Claude Code, Codex CLI): se estão
    /// ligadas, os diretórios exatos que leem (ou leriam) e o que o índice
    /// já segue; não lê logs. Com --json, em JSON. Tudo desligado por padrão.
    #[arg(long, group = "cost_action")]
    cost_sources: bool,
    /// Liga a leitura dos logs locais de FONTE (claude_code ou codex_cli):
    /// mostra exatamente quais diretórios serão lidos, para quê e que só
    /// números (horário, modelo, contagens de tokens, resumo anônimo da
    /// sessão) são extraídos, e pede confirmação no terminal. Nunca lê o
    /// conteúdo das conversas.
    #[arg(long, value_name = "FONTE", group = "cost_action")]
    cost_enable: Option<String>,
    /// Com --cost-enable, lê DIR em vez dos diretórios padrão (repetível);
    /// sem ele, volta aos padrão.
    #[arg(long, value_name = "DIR", requires = "cost_enable")]
    cost_path: Vec<PathBuf>,
    /// Desliga FONTE: nenhum log dela é lido e tudo o que foi derivado
    /// dela é apagado do índice.
    #[arg(long, value_name = "FONTE", group = "cost_action")]
    cost_disable: Option<String>,
    /// Apaga o índice local de custo inteiro (as fontes continuam ligadas
    /// ou desligadas como estão).
    #[arg(long, group = "cost_action")]
    cost_erase: bool,
    /// Lê agora, em primeiro plano, o que cresceu nos logs das fontes
    /// ligadas e diz o que encontrou; com --json, em JSON. Saída 0; 2 se
    /// nenhuma fonte está ligada ou uma ficou sem logs ou sem acesso; 1 em
    /// erro local.
    #[arg(long, group = "cost_action")]
    cost_scan: bool,
    /// Mostra os preços (USD por milhão de tokens) que a estimativa de
    /// custo local usa: a tabela embutida, com data e fonte, sob o seu
    /// pricing.json (revisado à mão, nunca baixado); com --json, em JSON.
    /// Saída 0; 1 se o pricing.json é inválido (fica ignorado). Não lê logs.
    #[arg(long, group = "cost_action")]
    cost_prices: bool,
    /// Com --cost-prices, imprime um fragmento de pricing.json com estes
    /// MODELOS e preços null para preencher (a data é hoje); nunca sugere
    /// um preço.
    #[arg(long, value_name = "MODELO", num_args = 1.., requires = "cost_prices")]
    template: Vec<String>,
    /// Guarda o preço do plano de uma conta (digitado por você, só neste
    /// computador): CONTA é provedor:nome (ex.: claude:pessoal), PREÇO o
    /// valor, MOEDA o código de três letras e DIA (1 a 28, padrão 1) o
    /// início do ciclo. Compara a estimativa a preços de API com o plano.
    #[arg(
        long,
        value_names = ["CONTA", "PREÇO", "MOEDA", "DIA"],
        num_args = 3..=4,
        group = "cost_action"
    )]
    cost_plan: Option<Vec<String>>,
    /// Remove o preço do plano de CONTA (provedor:nome).
    #[arg(long, value_name = "CONTA", group = "cost_action")]
    cost_plan_clear: Option<String>,
    /// Segunda moeda só para exibir: `--cost-currency BRL 5.42` (1 USD =
    /// 5,42 BRL, taxa digitada por você, nunca baixada) ou
    /// `--cost-currency off`. Os totais continuam em USD.
    #[arg(
        long,
        value_names = ["MOEDA", "TAXA"],
        num_args = 1..=2,
        group = "cost_action"
    )]
    cost_currency: Option<Vec<String>>,
    /// Mostra o custo estimado a preços de API do uso local das fontes
    /// ligadas: hoje, ontem e o período salvo (ou os últimos --days N
    /// dias), por modelo e por dia, depois de ler em primeiro plano o que
    /// cresceu nos logs; com --json, em JSON. É uma estimativa, não uma
    /// cobrança. Saída 0; 2 se nenhuma fonte está ligada ou uma ficou sem
    /// logs ou sem acesso; 1 em erro local.
    #[arg(long, group = "cost_action")]
    cost: bool,
    /// Com --cost, os últimos DIAS dias locais, hoje incluído (1 a 730),
    /// em vez do período salvo.
    #[arg(long, value_name = "DIAS", requires = "cost")]
    days: Option<u32>,
    /// Período do custo local mostrado: today, last_7_days (padrão),
    /// last_30_days, current_month ou all.
    #[arg(long, value_name = "PERÍODO", conflicts_with_all = COST_OPTION_CONFLICTS)]
    cost_period: Option<String>,
    /// Dias mantidos no índice de custo local (7 a 730; padrão 120).
    #[arg(long, value_name = "DIAS", conflicts_with_all = COST_OPTION_CONFLICTS)]
    cost_retention_days: Option<u32>,
}

/// `--cost-period`/`--cost-retention-days` only save preferences.
const COST_OPTION_CONFLICTS: [&str; 8] = [
    "cost_action",
    "once",
    "json",
    "json_v1",
    "history",
    "notify",
    "serve",
    "watch",
];

/// PRD 0015: the local cost action the flags ask for, if any.
fn cost_action(args: &mut Args) -> Option<cost::CostAction> {
    use cost::CostAction;
    if args.cost_sources {
        return Some(CostAction::Sources);
    }
    if let Some(source) = args.cost_enable.take() {
        let paths = std::mem::take(&mut args.cost_path);
        return Some(CostAction::Enable { source, paths });
    }
    if let Some(source) = args.cost_disable.take() {
        return Some(CostAction::Disable(source));
    }
    if args.cost_erase {
        return Some(CostAction::Erase);
    }
    if args.cost_scan {
        return Some(CostAction::Scan);
    }
    if let Some(values) = args.cost_plan.take() {
        let mut values = values.into_iter();
        return Some(CostAction::Plan {
            account: values.next().unwrap_or_default(),
            price: values.next().unwrap_or_default(),
            currency: values.next().unwrap_or_default(),
            cycle_day: values.next(),
        });
    }
    if let Some(account) = args.cost_plan_clear.take() {
        return Some(CostAction::PlanClear(account));
    }
    if let Some(values) = args.cost_currency.take() {
        let mut values = values.into_iter();
        return Some(CostAction::Currency {
            code: values.next().unwrap_or_default(),
            rate: values.next(),
        });
    }
    if args.cost_prices {
        return Some(if args.template.is_empty() {
            CostAction::Prices
        } else {
            CostAction::PricesTemplate(std::mem::take(&mut args.template))
        });
    }
    if args.cost {
        return Some(CostAction::Usage { days: args.days });
    }
    (args.cost_period.is_some() || args.cost_retention_days.is_some()).then(|| {
        CostAction::Options {
            period: args.cost_period.take(),
            retention_days: args.cost_retention_days,
        }
    })
}

/// PRD 0021 RF8.1: report package ownership without loading configuration or
/// contacting a provider/package manager.
fn print_install_source() -> io::Result<()> {
    let executable = std::env::current_exe()?;
    let source = detect_install_source(
        &executable,
        |key| std::env::var_os(key),
        || executable_dir_writable(&executable),
    );
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "source={}", source.as_str())?;
    writeln!(
        stdout,
        "updater_disabled_reason={}",
        source.updater_disabled_reason().unwrap_or("none")
    )?;
    writeln!(
        stdout,
        "upgrade_command={}",
        source.upgrade_command().unwrap_or("none")
    )
}

fn executable_dir_writable(executable: &Path) -> bool {
    let Some(directory) = executable.parent() else {
        return false;
    };
    let probe = directory.join(format!(".cuotai-write-probe-{}", std::process::id()));
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
}

/// PRD 0011 RF5: public status queries are opt-in (`--status`, or
/// `CUOTAI_STATUS` with any non-empty value except `0`); `--no-status`
/// always turns them off.
fn status_enabled(status: bool, no_status: bool, var: impl Fn(&str) -> Option<String>) -> bool {
    let set = var("CUOTAI_STATUS").is_some_and(|v| !v.is_empty() && v != "0");
    !no_status && (status || set)
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
    let mut args = match Args::try_parse() {
        Ok(args) => args,
        // `--help`/`--version` succeed; an invalid argument is a local error
        // (1), never the 2 that `--once`/`--json`/`--serve` keep for accounts.
        Err(error) => {
            error.print()?;
            return Ok(u8::from(error.use_stderr()));
        }
    };
    if args.install_source {
        print_install_source()?;
        return Ok(0);
    }
    if args.list_providers {
        for provider in Provider::ALL {
            println!(
                "{:<20} {}{}",
                serde_json::to_value(provider)?.as_str().unwrap_or_default(),
                provider.label(),
                // PRD 0036 RF9.2: an aggregator relays several upstreams.
                if provider.is_aggregator() {
                    " (aggregator)"
                } else {
                    ""
                }
            );
        }
        return Ok(0);
    }
    // PRD 0019 RF7.7: the embedded changelog, without loading the
    // accounts' configuration and without touching the network.
    if let Some(version) = args.changelog.take() {
        return changelog_view::print(version.as_deref(), ui::tui_locale());
    }
    // PRD 0017 RF2.4: one run of an approved extension, without loading
    // the accounts' configuration.
    if let Some(id) = args.extension_check.take() {
        return extension_check::run(&id, args.json).await;
    }
    if let Some(action) = cost_action(&mut args) {
        // PRD 0029 RF2: plans are keyed by the account id when a
        // configuration can be read; the cost commands work without one.
        let configured = ConfigFile::load(args.config.clone(), false)
            .ok()
            .map(|file| file.config.accounts);
        return Ok(cost::run(action, args.json, ui::tui_locale(), configured)?);
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
    let interactive = !args.once
        && !args.json
        && !args.json_v1
        && !args.serve
        && !args.watch
        && args.report_problem.is_none()
        && !args.canary;
    let status = status_enabled(args.status, args.no_status, |key| std::env::var(key).ok());
    if interactive && (!io::stdout().is_terminal() || !io::stdin().is_terminal()) {
        return Err(
            "A TUI requer terminal interativo. Use --once, --json, --json-v1, --serve ou --watch."
                .into(),
        );
    }
    let locale = ui::tui_locale();
    // The browser page after a sign-in callback speaks the TUI's language
    // and follows the browser's light/dark preference.
    cuotai_core::oauth::callback_page::set_callback_style(
        cuotai_core::oauth::callback_page::CallbackStyle {
            locale: locale.to_string(),
            theme: None,
        },
    );
    let events = args
        .events
        .as_ref()
        .map(|names| cuotai_core::watch::parse_events(names.iter().map(String::as_str)))
        .transpose()
        .map_err(|error| error.resolve(locale))?;
    let retention = retention(args.history_days)?;
    let file = ConfigFile::load(args.config, interactive)?;
    // PRD 0013 RF3.2: no hook runs before its exact commands were
    // confirmed, and no query starts before that is settled.
    let hooks = if args.hooks {
        Some(watch::confirmed_hooks(&file.path, locale)?)
    } else {
        None
    };
    let session = Arc::new(match args.dev_credentials_sqlite {
        Some(path) => {
            let session = OAuthSession::development_sqlite(&path)?;
            eprintln!("AVISO: credenciais em SQLite SEM CRIPTOGRAFIA (desenvolvimento); Keychain não será usado. Não compartilhe o banco.");
            session
        }
        None => OAuthSession::system()?,
    });
    if args.canary {
        return canary::run(
            &file.config,
            canary::CanaryArgs {
                out: args.out.take(),
                dry_run: args.dry_run,
                open: args.canary_open,
                include_metered: args.canary_include_metered,
            },
            session,
        )
        .await;
    }
    if let Some(name) = args.report_problem.take() {
        return report_problem::run(&file.config, &name, session).await;
    }
    if args.once || args.json || args.json_v1 || args.serve || args.watch {
        // PRD 0013 RF2.1: `--interval` holds for this run only.
        let config = match args.interval {
            Some(seconds) => cuotai_core::watch::with_interval(&file.config, seconds)
                .map_err(|error| error.resolve(locale))?,
            None => file.config.clone(),
        };
        let history = history_store(&file, retention);
        let mut monitor =
            Monitor::with_oauth(config.clone(), Arc::new(EnvironmentCredentials), session)?;
        monitor.incidents().set_enabled(status);
        if let Some(store) = history {
            monitor = monitor.with_history(store);
        }
        monitor.mark_history_removed();
        if args.serve || args.watch {
            let serve = args.serve.then_some(serve::ServeArgs {
                port: args.port,
                token_file: args.token_file,
                no_token: args.no_token,
            });
            let watch = args.watch.then_some(watch::WatchArgs {
                events,
                jsonl: args.jsonl,
                hooks,
                projected_lead: lead(args.projected_lead.as_deref()),
            });
            // PRD 0015 RF4.3: the server publishes `local_cost`, kept
            // current by the background scan while it runs.
            let local_cost = args.serve.then(cost::LocalCost::start).flatten();
            return headless::run(
                config,
                file.identity()?,
                monitor,
                serve,
                watch,
                local_cost,
                args.strict_exit,
            )
            .await;
        }
        // PRD 0010 RF2.3: read before this query's readings are recorded.
        let restored = if args.json_v1 {
            monitor.restored_readings()
        } else {
            Vec::new()
        };
        // PRD 0015 RF4.3: `local_cost` reads what grew in the logs of the
        // sources that are on (bounded, on its own thread) during the query.
        let local_cost = args
            .json_v1
            .then(|| std::thread::spawn(cost::one_shot_export));
        let snapshot = monitor.refresh().await;
        // The readings are queued off the refresh; let them land first.
        monitor.flush_history(HISTORY_FLUSH);
        let code = if snapshot.reports.iter().all(|r| r.status == Status::Ready) {
            0
        } else {
            2
        };
        if args.json {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer_pretty(&mut stdout, &snapshot)?;
            writeln!(stdout)?;
        } else if args.json_v1 {
            let export = single_export(
                &file.config,
                &file.identity()?,
                restored,
                snapshot,
                &monitor.incident_statuses(),
                Utc::now(),
            )
            .with_local_cost(local_cost.and_then(|job| job.join().ok()).flatten());
            let mut stdout = io::stdout().lock();
            serde_json::to_writer_pretty(&mut stdout, &export)?;
            writeln!(stdout)?;
        } else {
            let now = Utc::now();
            let recent: Vec<Vec<Vec<Point>>> = snapshot
                .reports
                .iter()
                .enumerate()
                .map(|(index, report)| {
                    monitor.recent_points(index, &report.metrics, report.checked_at)
                })
                .collect();
            print_snapshot(
                &file.config,
                &snapshot,
                &recent,
                &monitor.incident_statuses(),
                now,
            );
        }
        if let Some(warning) = monitor.history_status().and_then(|s| s.warning()) {
            eprintln!("cuotai: {}", warning.resolve(ui::tui_locale()));
        }
        return Ok(code);
    }
    let animations = motion::enabled(args.no_animations, |key| std::env::var(key).ok());
    // PRD 0015 RF2.2/RF4.2: the local cost index follows the logs of the
    // sources that are on from a thread of its own while the TUI is open,
    // and the account details show it.
    let local_cost = cost::LocalCost::start();
    ui::run(
        file,
        session,
        animations,
        retention,
        args.notify.then(|| lead(args.notify_projected.as_deref())),
        status,
        local_cost,
    )
    .await?;
    Ok(0)
}

/// PRD 0024: the lead of `--notify-projected`/`--projected-lead` (clap
/// accepts only 15, 30 or 60).
fn lead(minutes: Option<&str>) -> Option<cuotai_core::alerts::Lead> {
    cuotai_core::alerts::Lead::from_minutes(minutes?.parse().ok()?)
}

/// PRD 0012 RF4.3: the `--json-v1` export of one run: `snapshot` applied to a
/// new cycle over the `restored` readings (PRD 0010 RF2.3, in account
/// order), so an account whose query failed carries its last recorded
/// reading as `last_good` with `stale`, as the server would publish it.
fn single_export(
    config: &Config,
    identity: &ConfigIdentity,
    restored: Vec<Option<Report>>,
    snapshot: Snapshot,
    incidents: &[ProviderStatus],
    now: DateTime<Utc>,
) -> Export {
    let mut cycle = RefreshCycle::new(config, Instant::now());
    cycle.restore_last_good_at(restored, now);
    if let Some(token) = cycle.begin_full() {
        cycle.finish_full(Some(snapshot), token, Instant::now());
    }
    Export::from_cycle(config, identity, &cycle, incidents, now, Options::default())
}

fn print_snapshot(
    config: &Config,
    snapshot: &Snapshot,
    recent: &[Vec<Vec<Point>>],
    incidents: &[ProviderStatus],
    now: DateTime<Utc>,
) {
    println!("CUOTAI • {}", snapshot.finished_at.to_rfc3339());
    print!(
        "\n{}",
        summary_lines(snapshot, ui::tui_locale(), ui::local_time)
    );
    for (index, report) in snapshot.reports.iter().enumerate() {
        let recent = recent.get(index).map_or(&[][..], Vec::as_slice);
        let status = incidents.iter().find(|s| s.provider == report.provider);
        // The snapshot keeps the configuration's account order.
        let interval = config.accounts.get(index).map_or(
            Duration::from_secs(config.refresh_seconds),
            |account| {
                Duration::from_secs(account.effective_refresh_seconds(config.refresh_seconds))
            },
        );
        print!(
            "{}",
            report_text(
                report,
                recent,
                status,
                interval,
                now,
                ui::tui_locale(),
                ui::local_time
            )
        );
    }
}

/// One `--once` account block: heading, the provider's flagged status (PRD
/// 0011 RF5, the card's rule), metrics (with the PRD 0007 RF4 pace line
/// under each eligible one, projected from the recorded `recent` points of
/// each metric when there are enough, at the reading's `checked_at` and only
/// while it is fresh for the account's `interval`, PRD 0024 FIX-11), notes
/// and source.
fn report_text(
    report: &Report,
    recent: &[Vec<Point>],
    status: Option<&ProviderStatus>,
    interval: Duration,
    now: DateTime<Utc>,
    locale: &str,
    at: impl Fn(DateTime<Utc>) -> String,
) -> String {
    let mut out = format!("\n{}\n", report_heading(report, now));
    if let Some(line) = status.and_then(|status| incident_line(status, locale, now, &at)) {
        out += &line;
    }
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
        if let Some(pace) = pace::assess_reading(metric, report.checked_at, interval, now, points) {
            let text = dashboard::pace_text(&pace, locale, &at);
            out += &format!("    Ritmo: {text} (estimativa linear)\n");
        }
    }
    out += &sub_reports_text(report, locale, &at);
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

/// PRD 0036 RF9.3: the block of an aggregator account, indented under its
/// line: one line per pool ("Claude · 2 de 3 disponíveis"), then every
/// connection with its status and metrics. Empty for any other account.
fn sub_reports_text(
    report: &Report,
    locale: &str,
    at: &impl Fn(DateTime<Utc>) -> String,
) -> String {
    if report.sub_reports.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for pool in cuotai_core::aggregators::pools(report) {
        out += &format!("  Pool: {}\n", ui::safe(&pool.summary().resolve(locale)));
    }
    for sub in &report.sub_reports {
        let name = sub.provider.map_or_else(
            || sub.upstream.clone(),
            |provider| provider.label().to_string(),
        );
        let mut flags = Vec::new();
        if sub.enabled_upstream == Some(false) {
            flags.push("desligada no 9router".to_string());
        }
        if sub.duplicate_of.is_some() {
            flags.push("também monitorada diretamente".to_string());
        }
        if let Some(plan) = &sub.plan {
            flags.push(format!("plano {}", ui::safe(plan)));
        }
        let flags = if flags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", flags.join("; "))
        };
        out += &format!(
            "  • {} · {} — {}{}\n",
            ui::safe(&name),
            ui::safe(&sub.label),
            sub.status.label(),
            flags
        );
        for metric in &sub.metrics {
            out += &format!(
                "      {}: {}\n",
                ui::safe(&metric.label),
                ui::metric_text(metric)
            );
            if let Some(reset) = metric.resets_at {
                out += &format!("        Renova: {}\n", at(reset));
            }
        }
        if sub.status != cuotai_core::Status::Ready {
            // The reason a connection has no reading; the two standing
            // caveats ("cached", "zero") are shown in the details instead.
            let reason = |note: &&cuotai_core::text::Localized| {
                let id = note.id.as_ref();
                id.starts_with("query.")
                    || (id.starts_with("note.nine_router.")
                        && !id.ends_with("cached")
                        && !id.ends_with("zero_caveat"))
            };
            if let Some(note) = sub.notes_l10n.iter().find(reason) {
                out += &format!("      {}\n", ui::safe(&note.resolve(locale)));
            }
        }
    }
    out
}

/// PRD 0011 RF5: `--once` line of a flagged provider status: state (or an
/// unreadable page), incident title, start and the page to open.
fn incident_line(
    status: &ProviderStatus,
    locale: &str,
    now: DateTime<Utc>,
    at: impl Fn(DateTime<Utc>) -> String,
) -> Option<String> {
    let badge = dashboard::incident_badge(status, locale, now)?;
    let mut parts = vec![badge.label];
    parts.extend(badge.title);
    if let Some(start) = status.incident.as_ref().and_then(|i| i.started_at) {
        parts.push(format!("desde {}", at(start)));
    }
    parts.push(ui::safe(&badge.url).into_owned());
    Some(format!("  Status do provedor: {}\n", parts.join(" · ")))
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
        "{} / {} — {} · consultado {}{}",
        ui::safe(&report.account),
        report.provider.label(),
        report.status.label(),
        ui::age_text(report.checked_at, now),
        ui::data_as_of_suffix(report)
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

    /// PRD 0024 RF3.4/RF5.1: `--notify` alone never projects;
    /// `--notify-projected` needs `--notify` and takes 15, 30 or 60 (60
    /// without a value); `--projected-lead` needs `--watch`.
    #[test]
    fn projected_flags() {
        use cuotai_core::alerts::Lead;
        let parse = |argv: &[&str]| {
            Args::try_parse_from(std::iter::once("cuotai").chain(argv.iter().copied()))
        };
        let notify = parse(&["--notify"]).unwrap();
        assert_eq!(lead(notify.notify_projected.as_deref()), None);
        let bare = parse(&["--notify", "--notify-projected"]).unwrap();
        assert_eq!(lead(bare.notify_projected.as_deref()), Some(Lead::Min60));
        let fifteen = parse(&["--notify", "--notify-projected", "15"]).unwrap();
        assert_eq!(lead(fifteen.notify_projected.as_deref()), Some(Lead::Min15));
        assert!(parse(&["--notify-projected", "30"]).is_err());
        assert!(parse(&["--notify", "--notify-projected", "45"]).is_err());
        let watch = parse(&["--watch", "--projected-lead", "30"]).unwrap();
        assert_eq!(lead(watch.projected_lead.as_deref()), Some(Lead::Min30));
        assert!(parse(&["--projected-lead", "30"]).is_err());
    }

    /// The default refresh interval of the `--once` block tests.
    const FIVE_MIN: Duration = Duration::from_secs(300);
    use chrono::TimeZone;
    use cuotai_core::{Metric, MetricKind, MetricWindow};

    fn quota(window: MetricWindow, used: f64, resets_at: Option<DateTime<Utc>>) -> Metric {
        Metric {
            billing: Default::default(),
            scope: None,
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
            account_id: None,
            diagnostic: None,
            account: account.into(),
            provider: Provider::Claude,
            checked_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            status,
            metrics,
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            sub_reports: Vec::new(),
            as_of: None,
        }
    }

    /// PRD 0012 RF4.3: `--json-v1` publishes this run's reports and, for an
    /// account whose query failed, its last recorded reading as stale.
    #[test]
    fn json_v1_keeps_the_recorded_reading_of_a_failed_account() {
        let config = Config::from_toml(
            "[[accounts]]\nname = \"Claude\"\nprovider = \"claude\"\n\n\
             [[accounts]]\nname = \"Work\"\nprovider = \"claude\"\n",
        )
        .unwrap();
        let week = || vec![quota(MetricWindow::Weekly, 40.0, None)];
        let snapshot = Snapshot {
            started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            finished_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
            reports: vec![
                report("Claude", Status::Ready, week()),
                report("Work", Status::RateLimited, Vec::new()),
            ],
        };
        let recorded = vec![
            Some(report("Claude", Status::Ready, Vec::new())),
            Some(report("Work", Status::Ready, week())),
        ];
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 10, 0).unwrap();
        let identity = ConfigIdentity::Explicit("test".into());
        let export = single_export(&config, &identity, recorded, snapshot, &[], now);
        let json = serde_json::to_value(&export).unwrap();
        assert_eq!(json["schema"], 1);
        let (ready, failed) = (&json["accounts"][0], &json["accounts"][1]);
        assert_eq!(ready["report"]["status"], "ready");
        assert_eq!(ready["report"]["metrics"][0]["used"], 40.0);
        assert_eq!(ready["stale"], false);
        assert!(ready["last_good"].is_null());
        assert_eq!(
            failed["report"]["status"],
            serde_json::to_value(Status::RateLimited).unwrap()
        );
        assert_eq!(failed["stale"], true);
        assert_eq!(failed["last_good"]["metrics"][0]["used"], 40.0);
        assert_eq!(failed["last_good"]["age_seconds"], 600);
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

    /// PRD 0036 RF9.3: an aggregator account prints its pools and every
    /// connection indented under it; any other account prints nothing extra.
    #[test]
    fn once_prints_the_pools_and_connections_of_an_aggregator() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let sub = |id: &str, upstream: &str, provider, status, metrics: Vec<Metric>| {
            cuotai_core::SubReport {
                id: id.into(),
                upstream: upstream.into(),
                provider,
                label: format!("conn-{id}"),
                status,
                metrics,
                notes_l10n: Vec::new(),
                checked_at: now,
                as_of: None,
                enabled_upstream: Some(true),
                duplicate_of: None,
                plan: None,
                via: None,
            }
        };
        let mut router = report("home", Status::Ready, Vec::new());
        router.provider = Provider::NineRouter;
        router.sub_reports = vec![
            sub(
                "a",
                "claude",
                Some(Provider::Claude),
                Status::Ready,
                vec![quota(MetricWindow::Weekly, 40.0, None)],
            ),
            sub("b", "glm", None, Status::Unsupported, Vec::new()),
        ];
        let at = |t: DateTime<Utc>| t.format("%H:%M").to_string();
        let text = report_text(&router, &[], None, FIVE_MIN, now, "en-US", at);
        assert!(
            text.contains("  Pool: Claude · 1 of 1 available\n"),
            "{text}"
        );
        assert!(
            text.contains("  • Claude · conn-a — Atualizado\n"),
            "{text}"
        );
        assert!(text.contains("      Weekly: 40.00 % / 100.00\n"), "{text}");
        // An upstream Cuotai does not know shows its slug.
        assert!(text.contains("  • glm · conn-b — Indisponível\n"), "{text}");
        // Other accounts are unchanged.
        let plain = report("Work", Status::Ready, Vec::new());
        assert_eq!(
            report_text(&plain, &[], None, FIVE_MIN, now, "en-US", at),
            "\nWork / Claude — Atualizado · consultado agora\n"
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
            report_text(&report, &[], None, FIVE_MIN, now, "en-US", at),
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
        assert!(report_text(&report, &[], None, FIVE_MIN, now, "es", at)
            .contains("Ritmo: 6 puntos por encima del ritmo · se agota a las 18:49"));

        // PRD 0007 RF1.5 with the recorded history: the 5-hour window rose
        // 30 points in the last 40 minutes, so it runs out before the reset.
        let point = |minutes: i64, used: f64| Point {
            taken_at: now - chrono::Duration::minutes(minutes),
            used,
        };
        let recent = [vec![point(40, 10.0), point(20, 25.0), point(0, 40.0)]];
        assert!(
            report_text(&report, &recent, None, FIVE_MIN, now, "en-US", at).contains(
                "Ritmo: 20 points in reserve · runs out at 01:20, 40m 00s before the reset"
            )
        );
    }

    /// PRD 0024 FIX-11: `--once` evaluates the pace at the reading's
    /// `checked_at` and prints none for a reading that is not fresh.
    #[test]
    fn once_pace_follows_the_reading_time_and_freshness() {
        let mut report = report(
            "Work",
            Status::Ready,
            vec![quota(MetricWindow::Window5h, 40.0, None)],
        );
        let checked_at = report.checked_at;
        // Reset 2 hours after `now`; the reading is 90 minutes older.
        let now = checked_at + chrono::Duration::minutes(90);
        report.metrics[0].resets_at = Some(now + chrono::Duration::minutes(120));
        let at = |t: DateTime<Utc>| t.format("%H:%M").to_string();
        let hour = Duration::from_secs(3600);
        // Fresh at a 1-hour interval: at `checked_at` 30% of the window had
        // elapsed, so 40% used is 10 points above the pace (the wall clock
        // would say 20 points in reserve).
        assert!(report_text(&report, &[], None, hour, now, "en-US", at)
            .contains("Ritmo: 10 points above the pace"));
        // Stale at a 5-minute interval: the age only, no pace.
        let text = report_text(&report, &[], None, FIVE_MIN, now, "en-US", at);
        assert!(text.contains("consultado há"), "{text}");
        assert!(!text.contains("Ritmo"), "{text}");
    }

    /// RF5.2: opt-in through `--status` or `CUOTAI_STATUS`; `--no-status`
    /// overrides both.
    #[test]
    fn status_queries_are_opt_in_and_no_status_wins() {
        let env = |value: Option<&'static str>| {
            move |key: &str| {
                (key == "CUOTAI_STATUS")
                    .then_some(value)
                    .flatten()
                    .map(String::from)
            }
        };
        assert!(!status_enabled(false, false, env(None)), "off by default");
        assert!(status_enabled(true, false, env(None)));
        assert!(status_enabled(false, false, env(Some("1"))));
        assert!(!status_enabled(false, false, env(Some("0"))));
        assert!(!status_enabled(false, false, env(Some(""))));
        assert!(!status_enabled(true, true, env(Some("1"))));
        assert!(!status_enabled(false, true, env(Some("yes"))));
    }

    /// RF5.1 text snapshot: `--once` names a flagged provider status under
    /// the heading and keeps the core's incident note; an operational or
    /// plainly unknown provider prints nothing.
    #[test]
    fn once_prints_the_incident_and_its_note() {
        use cuotai_core::incidents::{Incident, IncidentState};
        use cuotai_core::text::Localized;
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let at = |t: DateTime<Utc>| t.format("%H:%M").to_string();
        let mut failed = report("Work", Status::Error, Vec::new());
        for note in [
            Localized::fixed(
                "query.transport",
                "Falha de rede ou tempo limite na consulta.",
            ),
            Localized::fixed(
                "note.incident.ongoing",
                "O provedor informa um incidente em andamento; a falha pode não ser da sua conta.",
            ),
        ] {
            failed.notes.push(note.fallback.to_string());
            failed.notes_l10n.push(note);
        }
        let outage = ProviderStatus {
            provider: Provider::Claude,
            state: IncidentState::MajorOutage,
            incident: Some(Incident {
                title: "Elevated errors on Claude Code".into(),
                started_at: Some(now - chrono::Duration::minutes(30)),
                url: "https://status.claude.com/incidents/abc123".into(),
            }),
            page_url: "https://status.claude.com/".into(),
            read_at: Some(now),
            stale: false,
            expired: false,
        };
        assert_eq!(
            report_text(&failed, &[], Some(&outage), FIVE_MIN, now, "en-US", at),
            "\nWork / Claude — Erro · consultado agora\n\
             \x20 Status do provedor: Major outage · Elevated errors on Claude Code · desde 23:30 · https://status.claude.com/incidents/abc123\n\
             \x20 Network failure or query timeout.\n\
             \x20 The provider reports an ongoing incident; the failure may not be from your account.\n"
        );

        let maintenance = ProviderStatus {
            state: IncidentState::Maintenance,
            incident: None,
            stale: true,
            ..outage.clone()
        };
        let ready = report("Work", Status::Ready, Vec::new());
        assert_eq!(
            report_text(&ready, &[], Some(&maintenance), FIVE_MIN, now, "pt-BR", at),
            "\nWork / Claude — Atualizado · consultado agora\n\
             \x20 Status do provedor: Em manutenção · última leitura · lido agora · https://status.claude.com/\n"
        );
        let unreadable = ProviderStatus {
            state: IncidentState::Unknown,
            read_at: None,
            ..maintenance.clone()
        };
        assert!(report_text(&ready, &[], Some(&unreadable), FIVE_MIN, now, "pt-BR", at)
            .contains("  Status do provedor: Página de status indisponível · https://status.claude.com/\n"));
        let expired = ProviderStatus {
            state: IncidentState::Unknown,
            incident: None,
            read_at: Some(now - chrono::Duration::minutes(31)),
            stale: false,
            expired: true,
            ..outage.clone()
        };
        assert!(
            report_text(&ready, &[], Some(&expired), FIVE_MIN, now, "pt-BR", at)
                .contains("Status do provedor: Status desconhecido · lido há 31 min")
        );
        for state in [IncidentState::Operational, IncidentState::Unknown] {
            let quiet = ProviderStatus {
                state,
                stale: false,
                ..unreadable.clone()
            };
            assert!(
                !report_text(&ready, &[], Some(&quiet), FIVE_MIN, now, "pt-BR", at)
                    .contains("Status do provedor")
            );
        }
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
