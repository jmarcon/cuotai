//! Ponte entre `cuotai-core` e a UI. Não contém regra de negócio: validação,
//! edição de contas, escrita do TOML, classificação de métricas e agendamento
//! vêm do core; aqui só há estado, DTOs e orquestração.
use cuotai_core::{
    account_edit::{
        self, build_account, credential_help, default_env, field_descriptors,
        visible_field_indices, AccountDraft, CredentialKind, DraftField, DraftValue,
        FieldDescriptorKind, FieldVisibility,
    },
    config_file::{ConfigFile, Discovery},
    login::{self, CompletedLogin, LoginEvent, LoginRequest},
    metrics::{classify_metric, classify_report, AccountClass, MetricState},
    oauth::{requires_client_registration, supports_login, OAuthSession},
    refresh::{Edit, RefreshCycle},
    Account, Config, EnvironmentCredentials, Metric, Monitor, Provider, Report, Status,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::{async_runtime::Mutex, AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};

fn l10n_to_json(note: &cuotai_core::text::Localized) -> String {
    serde_json::to_string(note).unwrap_or_else(|_| note.fallback.to_string())
}


fn refresh_edit_error(error: &cuotai_core::refresh::EditError) -> cuotai_core::text::Localized {
    match error {
        cuotai_core::refresh::EditError::Index(index) => cuotai_core::text::Localized::with_param(
            "edit_error.index_out_of_range",
            "index",
            *index,
            error.to_string(),
        ),
        cuotai_core::refresh::EditError::UnexpectedDiff => cuotai_core::text::Localized::fixed(
            "edit_error.refresh_cycle_diff",
            error.to_string(),
        ),
    }
}

const TICK: Duration = Duration::from_secs(5);
pub const EVENT: &str = "snapshot";

/// Canal de eventos do login (RF2): somente `LoginEvent` serializável do
/// core — URLs de autorização públicas, user_code de device flow e textos
/// seguros. Tokens e URLs de callback NUNCA trafegam aqui. O envelope
/// carrega o `attempt` da tentativa para que o frontend descarte eventos
/// de um forwarder velho (login já substituído por outro).
pub const LOGIN_EVENT: &str = "login";

#[derive(Clone, Serialize)]
pub struct LoginEnvelope<'a> {
    pub attempt: u64,
    #[serde(flatten)]
    pub event: &'a login::LoginEvent,
}

/// Login em andamento (RF2). O `CompletedLogin` (token opaco em memória)
/// fica SOMENTE aqui: nunca é serializado para a UI, sobrevive ao fechamento
/// do diálogo durante o commit e é descartado sem gravar no cofre se o
/// usuário cancelar antes de salvar.
struct PendingLogin {
    /// Identificador monotônico: a tarefa forwarder o carrega e só muta o
    /// `PendingLogin` se o slot atual ainda for o seu — protege contra um
    /// forwarder velho que termine depois de o usuário já ter iniciado outro.
    attempt: u64,
    provider: Provider,
    /// Conta em reautenticação (RF2.8); `None` = conta nova.
    index: Option<usize>,
    /// Códigos manuais (ex.: MiMo) digitados na UI. O oneshot devolve o
    /// veredito IMEDIATO do core (`SubmitCodeError`: fila cheia, fluxo
    /// encerrado, modo sem código) para a UI manter o código no campo;
    /// a validação em si chega depois via `LoginEvent`.
    code_tx: mpsc::Sender<(String, oneshot::Sender<Result<(), String>>)>,
    /// Cancela a tarefa forwarder, que derruba o driver (sem escrita no cofre).
    cancel_tx: Option<oneshot::Sender<()>>,
    /// A tarefa forwarder terminou (Finished/Failed): um novo `login_start`
    /// pode substituir este pendente.
    done: bool,
    /// Presente após `Finished` com credencial gravável (modo oauth). Modos
    /// aws_sso/bailian_console terminam sem credencial no cofre Cuotai.
    completed: Option<CompletedLogin>,
}

/// What the scheduler decided to do on this tick. Pulled out of the
/// inner lock so the match arm below can call async methods on `self`
/// without re-entering the mutex.
/// A primeira rodada completa ao abrir o app. `RefreshCycle::new` agenda o primeiro prazo de cada
/// conta para daqui a um intervalo; esperar esse prazo deixava a tela em "consultando" por minutos.
/// Só vale enquanto nenhuma rodada completa foi tentada: reconsultas seguem os prazos por conta.
fn needs_initial_sweep(
    cycle: &RefreshCycle,
    tokens: &[Option<cuotai_core::refresh::Generation>],
) -> bool {
    cycle.snapshot().is_none()
        && !cycle.attempted_full()
        && tokens.iter().all(Option::is_none)
        && cycle.pending_indices().len() == cycle.len()
}

enum SchedulerPlan {
    /// Nothing to do this tick: no accounts, no pending slots, no due
    /// intervals.
    Idle,
    /// Initial sweep: every slot is pending and no full refresh is
    /// running. This is the only path that should ever do a full
    /// round of paid-provider queries.
    Full,
    /// Per-account refreshes driven by either an edit (pending slot)
    /// or a due periodic interval.
    Accounts(Vec<usize>),
}

struct Loaded {
    file: ConfigFile,
    monitor: Arc<Monitor>,
    cycle: RefreshCycle,
    /// Mirror of the cycle's `generation` counter; the inner mutex
    /// owns the value so concurrent `finish_*` calls can compare
    /// their token against the live one. The cycle is the source of
    /// truth for the generation (we read it via the cycle's own
    /// `begin` calls and forward the result), so this field mostly
    /// exists to detect "another edit landed while we were holding
    /// the lock" — if `Inner.generation` differs from
    /// `Loaded.generation`, an in-flight refresh's token is stale.
    generation: u64,
    /// Per-slot generation token for in-flight refreshes, mirroring
    /// the cycle's `in_flight` table. `None` means the slot is idle.
    /// Stored on `Loaded` so the inner mutex can quickly check "slot
    /// busy?" without going through the cycle.
    tokens: Vec<Option<cuotai_core::refresh::Generation>>,
}

#[derive(Default)]
struct Inner {
    loaded: Option<Loaded>,
    load_error: Option<String>,
    session: Option<Arc<OAuthSession>>,
    /// Login RF2 em andamento; no máximo um por vez (o diálogo é modal).
    login: Option<PendingLogin>,
    /// Contador monotônico: cada `login_start` recebe um id único que
    /// frente e back usam para casar `PendingLogin` com o que a UI vê.
    next_login_attempt: u64,
    generation: u64,
}

#[derive(Default)]
pub struct Backend {
    inner: Mutex<Inner>,
}

// ---------- DTOs (somente leitura para a UI) ----------

#[derive(Serialize, Clone)]
pub struct MetricView {
    #[serde(flatten)]
    metric: Metric,
    /// "unmeasured" | "exhausted" | "available" (regra do core).
    state: &'static str,
    /// Fração restante em [0,1] quando `available` e com limite utilizável.
    ratio: Option<f64>,
    uncapped: bool,
    no_cap: bool,
    /// "quota" (cota do plano) ou "rate_limit" (janela curta de limite de taxa).
    kind: &'static str,
    /// Janela da cota: "5h" (sessão de 5 horas) ou "week" (semanal); `None` para as demais (mensal, créditos…).
    window: Option<&'static str>,
}

#[derive(Serialize, Clone)]
pub struct Badge {
    /// "accent" | "info" | "good" (cor definida pela UI).
    tone: &'static str,
    text: String,
}

#[derive(Serialize, Clone)]
pub struct CopyValue {
    label: &'static str,
    value: String,
}

/// Metadados não secretos já presentes em `Account::params` (projetados pelo core no login).
#[derive(Serialize, Clone, Default)]
pub struct IdentityView {
    email: Option<String>,
    badges: Vec<Badge>,
    copy: Option<CopyValue>,
}

#[derive(Serialize, Clone)]
pub struct ReportView {
    index: usize,
    account: String,
    provider: Provider,
    checked_at: chrono::DateTime<chrono::Utc>,
    status: Status,
    metrics: Vec<MetricView>,
    notes: Vec<String>,
    /// RF8: sidecar estruturado paralelo a `notes` (mesma ordem, mesmo
    /// tamanho). A UI resolve `id` no catálogo e usa `fallback` (PT) em
    /// fallback. `skip_serializing_if = "Vec::is_empty"` mantém o JSON
    /// da CLI/Snapshot idêntico ao pré-RF8 quando não há notas.
    notes_l10n: Vec<cuotai_core::text::Localized>,
    source: String,
    class: &'static str,
    refresh_seconds: u64,
    /// Instante (Unix, ms) da próxima consulta agendada desta conta; `None` enquanto ela está sendo consultada.
    next_refresh_at_ms: Option<u64>,
    uses_oauth: bool,
    identity: IdentityView,
}

#[derive(Serialize, Clone)]
pub struct View {
    config_path: Option<String>,
    config_exists: bool,
    account_count: usize,
    /// `false` até a primeira rodada completa terminar.
    loaded: bool,
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    finished_at: Option<chrono::DateTime<chrono::Utc>>,
    reports: Vec<ReportView>,
    busy: Vec<usize>,
    error: Option<String>,
    /// Intervalo global de atualização (`refresh_seconds` do arquivo); 0 antes de carregar a configuração.
    refresh_seconds: u64,
}

/// Distingue a janela curta de rate limit da cota do plano. O core ainda não tem um tipo para isso
/// (PRD 0001, RF10): hoje só marca em texto livre (`period` e rótulo), então reconhecemos esses termos.
fn metric_kind(metric: &Metric) -> &'static str {
    let is_rate = |text: &str| {
        let t = text.to_lowercase();
        t.contains("rate limit") || t.contains("rate-limit") || t.contains("ratelimit")
    };
    if is_rate(&metric.label) || metric.period.as_deref().is_some_and(is_rate) {
        "rate_limit"
    } else {
        "quota"
    }
}

/// Minúsculas e sem acentos comuns do português, para casar rótulos como "Sessão" e "Semana".
fn fold(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

/// `needle` em `hay` sem dígito imediatamente antes ("5h" casa, "25h" não).
fn has_unprefixed(hay: &str, needle: &str) -> bool {
    hay.match_indices(needle).any(|(i, _)| {
        !hay[..i]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_digit())
    })
}

/// Janela de 5 horas ou semanal de uma cota. O core ainda não informa a janela (PRD 0001, RF10.2): os adaptadores
/// só a descrevem em texto livre (`label`/`period`: "Sessão (5 horas)", "M-series (5h)", "Coding Plan: 5h",
/// "Semanal", "weekly (7d)"), então reconhecemos esses termos num único ponto. A janela de rate limit nunca conta.
fn metric_window(metric: &Metric) -> Option<&'static str> {
    if metric_kind(metric) == "rate_limit" {
        return None;
    }
    let text = fold(&format!(
        "{} {}",
        metric.label,
        metric.period.as_deref().unwrap_or("")
    ));
    let five = ["5h", "5 h", "5hour", "5-hour", "5 hour", "5 hora", "five hour", "five_hour"];
    let week = ["7d", "7 d", "7-day", "7 day", "7 dia"];
    if text.contains("session") || text.contains("sessao") || five.iter().any(|n| has_unprefixed(&text, n)) {
        Some("5h")
    } else if text.contains("semana")
        || text.contains("semanal")
        || text.contains("weekly")
        || text.contains("week")
        || week.iter().any(|n| has_unprefixed(&text, n))
    {
        Some("week")
    } else {
        None
    }
}

fn metric_view(metric: &Metric) -> MetricView {
    let (state, ratio, uncapped, no_cap) = match classify_metric(metric) {
        MetricState::Unmeasured => ("unmeasured", None, false, false),
        MetricState::Exhausted { no_cap } => ("exhausted", Some(0.0), false, no_cap),
        MetricState::Available { ratio, uncapped } => {
            ("available", (!uncapped).then_some(ratio), uncapped, false)
        }
    };
    MetricView {
        metric: metric.clone(),
        state,
        ratio,
        uncapped,
        no_cap,
        kind: metric_kind(metric),
        window: metric_window(metric),
    }
}

fn identity_view(account: &Account, status: Status) -> IdentityView {
    let param = |key: &str| {
        account
            .params
            .get(key)
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
    };
    let mut out = IdentityView::default();
    match account.provider {
        Provider::Kiro => {
            if let Some(method) = param("auth_method") {
                let text = match method {
                    "builder-id" => "AWS Builder ID",
                    "idc" => "IAM Identity Center",
                    other => other,
                };
                out.badges.push(Badge {
                    tone: "accent",
                    text: text.into(),
                });
            }
            if let Some(region) = param("region") {
                out.badges.push(Badge {
                    tone: "info",
                    text: region.into(),
                });
            }
            if status == Status::Ready {
                out.badges.push(Badge {
                    tone: "good",
                    text: "ativa".into(),
                });
            }
            out.copy = param("profile_arn").map(|v| CopyValue {
                label: "ARN do perfil",
                value: v.into(),
            });
        }
        Provider::CodeBuddy | Provider::Qoder | Provider::XiaomiMimo => {
            if let Some(region) = param("region") {
                out.badges.push(Badge {
                    tone: "info",
                    text: region.into(),
                });
            }
        }
        Provider::Grok => out.email = param("email").map(str::to_owned),
        _ => {}
    }
    out
}

fn report_view(index: usize, report: &Report, config: &Config, next_refresh_at_ms: Option<u64>) -> ReportView {
    let account = &config.accounts[index];
    ReportView {
        index,
        account: report.account.clone(),
        provider: report.provider,
        checked_at: report.checked_at,
        status: report.status,
        metrics: report.metrics.iter().map(metric_view).collect(),
        notes: report.notes.clone(),
        notes_l10n: report.notes_l10n.clone(),
        source: report.source.clone(),
        class: match classify_report(Some(report)) {
            AccountClass::Available => "available",
            AccountClass::Exhausted => "exhausted",
            AccountClass::Unmeasured => "unmeasured",
        },
        refresh_seconds: account.effective_refresh_seconds(config.refresh_seconds),
        next_refresh_at_ms,
        uses_oauth: account.oauth.is_some(),
        identity: identity_view(account, report.status),
    }
}

impl Inner {
    fn view(&self) -> View {
        let Some(l) = &self.loaded else {
            return View {
                config_path: None,
                config_exists: false,
                account_count: 0,
                loaded: false,
                started_at: None,
                finished_at: None,
                reports: vec![],
                busy: vec![],
                error: self.load_error.clone(),
                refresh_seconds: 0,
            };
        };
        // Per-account report lookup that mirrors the cycle's slots. Even
        // when the public snapshot is hidden by an in-flight edit, every
        // previously-refreshed account still has a usable report so the
        // UI does not flash "no data" for unrelated accounts while a
        // single edit refreshes.
        let slot_reports: Vec<Option<Report>> = match l.cycle.snapshot() {
            Some(snapshot) => snapshot.reports.iter().cloned().map(Some).collect(),
            None => (0..l.cycle.len())
                .map(|index| l.cycle.account_report(index).cloned())
                .collect(),
        };
        // The "fully loaded" indicator only flips once every slot has a
        // report, matching the cycle's own gating of `snapshot()`.
        let loaded = slot_reports.iter().all(Option::is_some) || l.file.config.accounts.is_empty();
        let now = Instant::now();
        let unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let started_at = l.cycle.snapshot().map(|snapshot| snapshot.started_at);
        let finished_at = l.cycle.snapshot().map(|snapshot| snapshot.finished_at);
        let reports: Vec<ReportView> = slot_reports
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| {
                slot.as_ref()
                    .map(|report| {
                        // Próxima consulta como instante absoluto: a UI faz a contagem regressiva sem novos eventos.
                        let next = (!l.cycle.is_busy(i)).then(|| {
                            unix_ms + l.cycle.next_refresh_in(i, now).as_millis() as u64
                        });
                        report_view(i, report, &l.file.config, next)
                    })
            })
            .collect();
        View {
            config_path: Some(l.file.path.display().to_string()),
            config_exists: l.file.exists,
            account_count: l.file.config.accounts.len(),
            loaded,
            started_at,
            finished_at,
            reports,
            busy: l
                .tokens
                .iter()
                .enumerate()
                .filter_map(|(i, t)| t.is_some().then_some(i))
                .collect(),
            error: None,
            refresh_seconds: l.file.config.refresh_seconds,
        }
    }

    /// Reaproveita o `ConfigFile` já salvo (mantém o snapshot de bytes) com monitor/agenda novos.
    #[allow(dead_code)]
    fn reinstall(&mut self) -> Result<(), String> {
        let file = self
            .loaded
            .take()
            .ok_or("Configuração não carregada.")?
            .file;
        self.install(file)
    }

    fn install(&mut self, file: ConfigFile) -> Result<(), String> {
        let session = match &self.session {
            Some(s) => Arc::clone(s),
            None => {
                // Somente desenvolvimento: SQLite em texto puro, recusado por builds release (core).
                let session = match std::env::var_os("CUOTAI_DEV_CREDENTIALS_SQLITE") {
                    Some(path) => {
                        eprintln!("AVISO: credenciais em SQLite SEM CRIPTOGRAFIA (desenvolvimento); Keychain não será usado.");
                        OAuthSession::development_sqlite(std::path::Path::new(&path))
                    }
                    None => OAuthSession::system(),
                };
                let s = Arc::new(session.map_err(|_| "Cofre de credenciais indisponível.")?);
                self.session = Some(Arc::clone(&s));
                s
            }
        };
        let monitor = Arc::new(
            Monitor::with_oauth(
                file.config.clone(),
                Arc::new(EnvironmentCredentials),
                session,
            )
            .map_err(|e| serde_json::to_string(&cuotai_core::edit_error::config_error_note(&e)).unwrap_or_else(|_| cuotai_core::edit_error::config_error_note(&e).fallback.into_owned()))?,
        );
        self.generation += 1;
        let accounts_len = file.config.accounts.len();
        self.loaded = Some(Loaded {
            cycle: RefreshCycle::new(&file.config, Instant::now()),
            file,
            monitor,
            generation: self.generation,
            tokens: vec![None; accounts_len],
        });
        self.load_error = None;
        Ok(())
    }
}

fn explicit_path() -> Option<PathBuf> {
    std::env::var_os("CUOTAI_CONFIG").map(PathBuf::from)
}

impl Backend {
    async fn emit(&self, app: &AppHandle) {
        let view = self.inner.lock().await.view();
        let _ = app.emit(EVENT, view);
    }

    /// Carrega a configuração se ainda não houver uma válida em memória.
    async fn ensure_loaded(&self) {
        let mut g = self.inner.lock().await;
        if g.loaded.is_some() {
            return;
        }
        // RF4.1: graphical apps MUST NOT pick up a ./cuotai.toml
        // from the process cwd (whose meaning is unpredictable for a
        // backgrounded desktop app). Discovery is therefore SystemOnly
        // unless the user explicitly overrode the path via env.
        match ConfigFile::load_with(explicit_path(), true, Discovery::SystemOnly) {
            Ok(file) => match g.install(file) {
                Ok(()) => {}
                Err(e) => g.load_error = Some(e),
            },
            Err(e) => g.load_error = Some(e.to_string()),
        }
    }

    pub async fn view(&self) -> View {
        self.ensure_loaded().await;
        self.inner.lock().await.view()
    }

    /// Rodada completa. `force` ignora o intervalo (botão "Atualizar tudo").
    pub async fn refresh_all(&self, app: &AppHandle) {
        self.ensure_loaded().await;
        // Claim the sweep token BEFORE spawning the task: any edit
        // that bumps `cycle.generation` while the task is in flight
        // invalidates the token we capture here. `begin_full` returns
        // `None` when at least one slot is already busy with a
        // per-account refresh — in that case the scheduler should have
        // routed the sweep through the per-account path; we simply
        // bail.
        let (monitor, token) = {
            let mut g = self.inner.lock().await;
            let Some(l) = g.loaded.as_mut() else { return };
            let Some(token) = l.cycle.begin_full() else {
                return;
            };
            for slot in l.tokens.iter_mut() {
                *slot = Some(token);
            }
            (Arc::clone(&l.monitor), token)
        };
        self.emit(app).await;
        let snapshot = monitor.refresh().await;
        {
            let mut g = self.inner.lock().await;
            if let Some(l) = g.loaded.as_mut() {
                l.cycle.finish_full(Some(snapshot), token, Instant::now());
                for slot in l.tokens.iter_mut() {
                    if *slot == Some(token) {
                        *slot = None;
                    }
                }
            }
        }
        self.emit(app).await;
    }

    pub async fn refresh_account(&self, app: &AppHandle, index: usize) -> Result<(), String> {
        // Claim the per-slot token before spawn so an edit that lands
        // mid-flight rejects the eventual result via the generation
        // guard. `begin` returns `None` when the slot is already busy;
        // the caller can retry later.
        let (monitor, token) = {
            let mut g = self.inner.lock().await;
            let l = g.loaded.as_mut().ok_or("Configuração não carregada.")?;
            if index >= l.file.config.accounts.len() {
                return Err("Conta inexistente.".into());
            }
            // Allow refresh whenever the slot is pending OR the cycle
            // already has a snapshot — never bail out just because the
            // public snapshot is hidden by a partial edit.
            let Some(token) = l.cycle.begin(index) else {
                return Ok(());
            };
            l.tokens[index] = Some(token);
            (Arc::clone(&l.monitor), token)
        };
        self.emit(app).await;
        let report = monitor.refresh_account(index).await;
        {
            let mut g = self.inner.lock().await;
            if let Some(l) = g.loaded.as_mut() {
                l.cycle
                    .finish_account(index, Some(report), token, Instant::now());
                if let Some(slot) = l.tokens.iter_mut().find(|slot| **slot == Some(token)) {
                    *slot = None;
                }
            }
        }
        self.emit(app).await;
        Ok(())
    }

    /// Laço de fundo: primeira rodada completa e depois cada conta no seu
    /// intervalo. After a partial edit, the cycle has a hidden snapshot
    /// (pending slots) and the scheduler must only fire the per-account
    /// refreshes that are actually due — never a full sweep, which would
    /// re-query every account.
    ///
    /// The scheduler respects the cycle's per-slot backoff: a failed
    /// task pushes the deadline forward; the scheduler skips the slot
    /// until the deadline has elapsed (see
    /// [`RefreshCycle::due_indices`] and
    /// [`RefreshCycle::pending_now_indices`]).
    pub async fn run_scheduler(self: Arc<Self>, app: AppHandle) {
        loop {
            self.ensure_loaded().await;
            let plan: SchedulerPlan = 'plan: {
                let g = self.inner.lock().await;
                let Some(l) = g.loaded.as_ref() else {
                    // Sem configuração válida: espera o próximo tick (nada de laço apertado).
                    break 'plan SchedulerPlan::Idle;
                };
                if l.file.config.accounts.is_empty() {
                    SchedulerPlan::Idle
                } else {
                    let now = Instant::now();
                    let can_full = needs_initial_sweep(&l.cycle, &l.tokens);
                    if can_full {
                        // Primeira rodada: sem dados, nenhuma rodada completa
                        // tentada e nada em andamento. Roda já, sem esperar
                        // o prazo periódico (que só vale para as próximas).
                        SchedulerPlan::Full
                    } else {
                        // Deadline-aware pending (edit-driven) + periodic
                        // due (Some-slot, deadline elapsed). Both skip
                        // slots that already have an in-flight refresh.
                        let mut all: Vec<usize> = l
                            .cycle
                            .pending_now_indices(now)
                            .into_iter()
                            .chain(l.cycle.due_indices(now))
                            .collect();
                        all.retain(|i| l.tokens.get(*i).and_then(Option::as_ref).is_none());
                        all.sort_unstable();
                        all.dedup();
                        if all.is_empty() {
                            SchedulerPlan::Idle
                        } else {
                            SchedulerPlan::Accounts(all)
                        }
                    }
                }
            };
            match plan {
                SchedulerPlan::Idle => {}
                SchedulerPlan::Full => self.refresh_all(&app).await,
                SchedulerPlan::Accounts(indexes) => {
                    for index in indexes {
                        let (b, a) = (Arc::clone(&self), app.clone());
                        tauri::async_runtime::spawn(async move {
                            let _ = b.refresh_account(&a, index).await;
                        });
                    }
                }
            }
            tokio::time::sleep(TICK).await;
        }
    }

    // ---------- edição de contas ----------

    pub async fn save_account(&self, app: &AppHandle, input: AccountInput) -> Result<(), String> {
        self.ensure_loaded().await;
        // Everything below runs under ONE inner.lock so a concurrent
        // refresh / scheduler tick cannot observe a half-applied edit.
        // The commit path holds the lock across `commit_login`'s vault
        // write (spawn_blocking against the local vault, brief): the
        // desktop is single-user with a modal dialog, and keeping the
        // lock avoids any half-applied window between the vault and
        // TOML stages.
        let refresh_target: Option<usize> = {
            let mut g = self.inner.lock().await;
            // Split borrows: pending login, session and loaded config are
            // disjoint fields.
            let Inner {
                login,
                loaded,
                session,
                generation,
                ..
            } = &mut *g;
            let l = loaded.as_mut().ok_or("Configuração não carregada.")?;
            // RF2: a `commit_login` request deve encontrar o pending login
            // de THIS account/provider, da MESMA tentativa (o `attempt` evita
            // que um forwarder velho confunda um login já substituído por
            // outro). O `CompletedLogin` (token opaco) nunca sai de
            // `PendingLogin`. Modos sem credencial Cuotai (aws_sso /
            // bailian_console) são consumidos pelo save sem escrita no
            // cofre.
            let pending_match_ref = login.as_ref().filter(|p| {
                p.provider == input.provider
                    && p.index == input.index
                    && p.attempt == input.login_attempt
            });
            if input.commit_login {
                let Some(p) = pending_match_ref else {
                    return Err(
                        "Autorização pendente não encontrada para esta conta; refaça o login."
                            .into(),
                    );
                };
                if !p.done {
                    return Err("Login em andamento; conclua ou cancele antes de salvar.".into());
                }
            }
            let completed: Option<&CompletedLogin> = pending_match_ref
                .filter(|_| input.commit_login)
                .and_then(|p| p.completed.as_ref());
            let config = &l.file.config;
            let existing = input.index.and_then(|i| config.accounts.get(i));
            let had_oauth = existing
                .filter(|a| a.provider == input.provider)
                .is_some_and(|a| a.oauth.is_some());
            // Modo de autenticação escolhido no diálogo. O frontend envia o
            // id do `LoginMode` (ou "credential") para que o backend
            // determine com clareza: credencial manual REMOVE o oauth
            // existente; modo OAuth preserva o oauth existente quando não
            // há novo login nesta sessão.
            let auth_mode_credential = input.auth_mode == "credential";
            let oauth_existing = match (auth_mode_credential, had_oauth) {
                // Trocar para credencial manual remove o oauth.
                (true, _) => None,
                // Edicao OAuth sem novo login: preserva a config existente.
                (false, true) => existing
                    .filter(|a| a.provider == input.provider)
                    .and_then(|a| a.oauth.as_ref())
                    .cloned(),
                // Conta sem oauth: nada a preservar.
                (false, false) => None,
            };
            // Habilita o modo OAuth SOMENTE quando há credencial Cuotai
            // (token gravado no cofre). Modos `aws_sso` e `bailian_console`
            // gravam fora do cofre Cuotai (AWS CLI cache / `bl` vault), e o
            // core exige `oauth_authorized` ou `oauth_existing` quando
            // `oauth_mode = true`. Marcar `oauth_mode = true` para esses
            // modos faria `build_account` falhar com "Faça login". A
            // credencial/opções deles vão nos campos do rascunho e em
            // `params` (Bailian adiciona `cli_profile` por conta do core).
            let oauth_mode = completed.is_some() || oauth_existing.is_some();
            // Modo de login do Cuotai escolhido, mas sem login concluído nem credencial existente: o core cobraria a
            // "variável de ambiente", que não existe nesse modo. Dizemos o motivo real.
            if login_needed_before_save(&input.auth_mode, oauth_mode) {
                return Err("Conclua o login antes de salvar (ou volte para a credencial manual).".into());
            }
            let descriptors = field_descriptors(input.provider);
            let fields: Vec<DraftField> = descriptors
                .iter()
                .filter_map(|d| {
                    let f = input.fields.iter().find(|f| f.key == d.key)?;
                    let value = match d.kind {
                        FieldDescriptorKind::Text => DraftValue::Text(f.text.clone()?),
                        FieldDescriptorKind::Choice(_) => DraftValue::Choice(f.choice?),
                    };
                    Some(DraftField { key: d.key, value })
                })
                .collect();
            // Opções nativas registradas neste salvamento (sem segredos) só
            // fluem se o login foi concluído; o core resolve o que
            // sobrescreve e o que mantém das `oauth_existing`/`params`.
            let oauth_login_params = completed.map(|c| c.params.clone()).unwrap_or_default();
            // Bailian stores its login in the `bl` profile; persist the exact
            // profile passed to the CLI. A missing profile is generated only
            // when saving without a login (the manual Cookie path).
            let credential_params = if input.auth_mode == login::MODE_BAILIAN_CONSOLE {
                let mut params = BTreeMap::new();
                let profile = input.cli_profile_override.trim();
                if profile.is_empty() && !input.commit_login {
                    account_edit::ensure_cli_profile(&mut params);
                } else if account_edit::is_valid_cli_profile(profile) {
                    params.insert("cli_profile".to_string(), profile.to_string());
                } else {
                    return Err("Perfil do `bl` inválido; use letras, números, '-' ou '_', até 32 caracteres.".into());
                }
                Some(params)
            } else {
                None
            };
            let draft = AccountDraft {
                provider: input.provider,
                name: &input.name,
                credential: &input.credential,
                oauth_mode,
                oauth_existing: oauth_existing.as_ref(),
                oauth_authorized: completed.map(|c| c.config.clone()),
                oauth_login_params,
                existing_params: None,
                credential_params,
                fields: &fields,
                interval_text: &input.interval,
            };
            let account = build_account(&draft, config, input.index)
                .map_err(|e| l10n_to_json(&e.l10n()))?;
            let updated = config
                .upsert_account(account, input.index)
                .map_err(|e| l10n_to_json(&e.l10n()))?;
            let prev = config.clone();
            let edit_index = input.index.unwrap_or(prev.accounts.len());
            // Stage the new cycle on a copy so a structural-diff rejection
            // (e.g. reorder) leaves the live state untouched.
            let mut staged = l.cycle.clone_state_for_edit();
            let due = staged
                .apply_edit(&prev, &updated, Edit::Upsert(edit_index), Instant::now())
                .map_err(|e| l10n_to_json(&refresh_edit_error(&e)))?;
            let refresh_target = due.first().copied();
            // Build the new monitor. A failure here means the config is
            // structurally invalid; we abort before touching disk.
            let new_monitor = match l.monitor.with_config(updated.clone()) {
                Ok(m) => Arc::new(m),
                Err(e) => {
                    return Err(l10n_to_json(&cuotai_core::edit_error::config_error_note(&e)));
                }
            };
            // Persist. O caminho de commit delega a escrita encenada
            // cofre→TOML ao core (RF2.4): cofre primeiro capturando o
            // token anterior (reauth), depois `ConfigFile::save`; uma
            // `PartialFailure` NUNCA apaga um token substituído sem
            // restaurá-lo. O caminho simples só toca o TOML.
            if let Some(completed) = completed {
                let session = session
                    .as_ref()
                    .map(Arc::as_ref)
                    .ok_or("Sessão OAuth não inicializada.")?;
                match login::commit_login(&mut l.file, session, completed, updated.clone()).await {
                    login::LoginCommitOutcome::Saved => {}
                    login::LoginCommitOutcome::PartialFailure(failure) => {
                        // Runtime fica na config anterior; o pendente NÃO é
                        // consumido, então "Salvar" refaz o commit sem novo
                        // OAuth. A mensagem do core carrega a verdade exata.
                        let wrapped = format!(
                            "{} A autorização continua em memória: clique em Salvar novamente para concluir sem refazer o login.",
                            failure.message
                        );
                        return Err(l10n_to_json(&cuotai_core::text::Localized::fixed(
                            "edit_error.login_partial_failure",
                            wrapped,
                        )));
                    }
                }
            } else if let Err(e) = l.file.save(updated.clone()) {
                // `save` roda `validate` de novo; falha aqui é I/O real e o
                // runtime continua na configuração anterior.
                return Err(l10n_to_json(&cuotai_core::edit_error::EditError::l10n(&e)));
            }
            // Persistido: pendente consumido (committed credential ou modo
            // sem cofre) some. Para `auth_mode_credential` sem oauth, o
            // frontend não envia `commitLogin` e o pendente, se houver, é
            // simplesmente descartado também.
            if pending_match_ref.is_some() {
                *login = None;
            }
            // Commit: cycle + monitor + bookkeeping + generation
            // mudam juntos sob a mesma aquisição do lock. Bump direto
            // em `l.generation`; `Inner.generation` espelha, então
            // propagamos por último (ainda sob o lock).
            l.cycle = staged;
            l.monitor = new_monitor;
            if l.tokens.len() < l.file.config.accounts.len() {
                l.tokens
                    .extend((0..l.file.config.accounts.len() - l.tokens.len()).map(|_| None));
            }
            if let Some(slot) = l.tokens.get_mut(edit_index) {
                *slot = None;
            }
            l.generation += 1;
            *generation = l.generation;
            refresh_target
        };
        self.emit(app).await;
        if let Some(index) = refresh_target {
            self.refresh_account(app, index).await?;
        }
        Ok(())
    }

    /// `purge_credentials`: apagar também a credencial OAuth do cofre do sistema. O core decide se pode (credencial
    /// compartilhada com outra conta nunca é apagada) e o resultado diz o que realmente aconteceu.
    pub async fn delete_account(
        &self,
        app: &AppHandle,
        index: usize,
        purge_credentials: bool,
    ) -> Result<DeleteResult, String> {
        self.ensure_loaded().await;
        // The core RF3 `remove_account` owns the save-then-purge
        // ordering and the shared-key guard. On the no-purge path
        // (the only path the desktop ever sends) the core performs no
        // `.await` — the only async point is `ConfigFile::save`, which
        // is fully synchronous — so we can hold the inner mutex
        // across the entire core call. This avoids the `Option::take`
        // pattern that would briefly expose `loaded = None` to a
        // concurrent `ensure_loaded`, which would then install a fresh
        // cycle / monitor only for our phase 3 commit to clobber it.
        // `purge_credentials = false` is the conscious desktop default
        // — the web UI never asks for a purge, so we keep the
        // no-purge path explicit without duplicating policy. When a
        // future revision sends `purge_credentials = true`, the IPC
        // round-trip will block the inner mutex; that is a real
        // trade-off the product needs to make explicitly.
        let outcome = {
            let mut g = self.inner.lock().await;
            // Clone the session Arc up front so we can drop the
            // `&g.session` immutable borrow before taking a mutable
            // borrow of `g.loaded` (the borrow checker rejects
            // overlapping immutable + mutable borrows on the same
            // `MutexGuard`).
            let session = match &g.session {
                Some(s) => Arc::clone(s),
                None => return Err("Sessão OAuth não inicializada.".into()),
            };
            let l = g.loaded.as_mut().ok_or("Configuração não carregada.")?;
            let prev = l.file.config.clone();
            if index >= prev.accounts.len() {
                return Err("Conta inexistente.".into());
            }
            let updated = prev
                .without_account(index)
                .map_err(|e| l10n_to_json(&cuotai_core::edit_error::EditError::l10n(&e)))?;
            let mut staged_cycle = l.cycle.clone_state_for_edit();
            staged_cycle
                .apply_edit(&prev, &updated, Edit::Remove(index), Instant::now())
                .map_err(|e| l10n_to_json(&refresh_edit_error(&e)))?;
            let new_monitor = l
                .monitor
                .with_config(updated)
                .map_err(|e| l10n_to_json(&cuotai_core::edit_error::config_error_note(&e)))?;
            // Drive the core removal under the SAME lock. The
            // no-purge path does not await IPC; the (future)
            // purge path will block the mutex until the vault
            // round-trip completes.
            let remove_outcome =
                account_edit::remove_account(&mut l.file, session.as_ref(), index, purge_credentials).await;
            let remove_outcome = match remove_outcome {
                Ok(outcome) => outcome,
                Err(error) => {
                    // On a pre-save failure the live `ConfigFile`
                    // (and therefore `l.file`) is unchanged, so we
                    // only need to surface the error. The staged
                    // cycle + monitor are dropped on return.
                    return Err(l10n_to_json(&account_edit::RemoveError::l10n(&error)));
                }
            };
            // Commit under the same lock: disk has been updated by
            // the core, mirror that into the runtime (cycle,
            // monitor, busy set, generation). The cycle's
            // `apply_edit(Remove)` already cleared `busy` /
            // `in_flight` / bumped `generation`; we mirror that
            // onto `Loaded.tokens` (drop the removed index and
            // clear the bookkeeping that lost its slot).
            l.cycle = staged_cycle;
            l.monitor = Arc::new(new_monitor);
            if index < l.tokens.len() {
                l.tokens.remove(index);
            }
            l.generation += 1;
            g.generation = l.generation;
            remove_outcome
        };
        // Remover não dispara consultas (RF1); o resultado diz o que aconteceu com o cofre, sem silêncio.
        let result = match outcome {
            account_edit::RemoveOutcome::RemovedNoCredential(_) => DeleteResult::new("removed"),
            account_edit::RemoveOutcome::RemovedFromVault(_) => DeleteResult::new("purged"),
            account_edit::RemoveOutcome::CredentialKept { reason, .. } => match reason {
                account_edit::KeptReason::Declined => DeleteResult::new("kept"),
                account_edit::KeptReason::SharedWith { other_account } => DeleteResult {
                    outcome: "kept_shared",
                    other_account: Some(other_account),
                    message: None,
                },
            },
            account_edit::RemoveOutcome::PartialFailure { vault_error, .. } => DeleteResult {
                outcome: "partial_failure",
                other_account: None,
                message: Some(vault_error.to_string()),
            },
        };
        self.emit(app).await;
        Ok(result)
    }

    /// Troca o intervalo global de atualização (`refresh_seconds`). Valida e grava pelo core; o ciclo é recriado com
    /// os mesmos relatórios (nenhuma consulta nova) e os prazos de todas as contas sem intervalo próprio passam a
    /// seguir o novo valor.
    pub async fn set_refresh_seconds(&self, app: &AppHandle, seconds: u64) -> Result<(), String> {
        self.ensure_loaded().await;
        {
            let mut g = self.inner.lock().await;
            let l = g.loaded.as_mut().ok_or("Configuração não carregada.")?;
            let mut updated = l.file.config.clone();
            updated.refresh_seconds = seconds;
            updated.validate().map_err(|e| {
                l10n_to_json(&cuotai_core::edit_error::config_error_note(&e))
            })?;
            let new_monitor = l
                .monitor
                .with_config(updated.clone())
                .map_err(|e| l10n_to_json(&cuotai_core::edit_error::config_error_note(&e)))?;
            l.file
                .save(updated.clone())
                .map_err(|e| l10n_to_json(&cuotai_core::edit_error::EditError::l10n(&e)))?;
            l.cycle = rebuild_cycle_keeping_reports(&l.cycle, &updated);
            l.monitor = Arc::new(new_monitor);
            l.tokens = vec![None; updated.accounts.len()];
            l.generation += 1;
            g.generation = l.generation;
        }
        self.emit(app).await;
        Ok(())
    }

    /// Persist a shared configuration-order move. `Edit::Move` carries every
    /// report and deadline to the account's new slot and returns no refresh
    /// targets, so this command deliberately never calls a provider.
    pub async fn move_account(
        &self,
        app: &AppHandle,
        from: usize,
        to: usize,
    ) -> Result<(), String> {
        self.ensure_loaded().await;
        {
            let mut g = self.inner.lock().await;
            let l = g.loaded.as_mut().ok_or("Configuração não carregada.")?;
            let prev = l.file.config.clone();
            let updated = prev
                .move_account(from, to)
                .map_err(|e| l10n_to_json(&cuotai_core::edit_error::EditError::l10n(&e)))?;
            if from == to {
                return Ok(());
            }
            let mut staged_cycle = l.cycle.clone_state_for_edit();
            let due = staged_cycle
                .apply_edit(
                    &prev,
                    &updated,
                    Edit::Move { from, to },
                    Instant::now(),
                )
                .map_err(|e| l10n_to_json(&refresh_edit_error(&e)))?;
            debug_assert!(due.is_empty(), "a move must not schedule queries");
            let new_monitor = l
                .monitor
                .with_config(updated.clone())
                .map_err(|e| l10n_to_json(&cuotai_core::edit_error::config_error_note(&e)))?;
            l.file
                .save(updated)
                .map_err(|e| l10n_to_json(&cuotai_core::edit_error::EditError::l10n(&e)))?;

            l.cycle = staged_cycle;
            l.monitor = Arc::new(new_monitor);
            // Keep busy indicators attached to account identity. Completion
            // clears these mirrored tokens by token value (not old index),
            // while the cycle rejects their now-stale reports safely.
            let token = l.tokens.remove(from);
            l.tokens.insert(to, token);
            l.generation += 1;
            g.generation = l.generation;
        }
        self.emit(app).await;
        Ok(())
    }

    pub async fn get_account(&self, index: usize) -> Result<AccountForm, String> {
        let g = self.inner.lock().await;
        let l = g.loaded.as_ref().ok_or("Configuração não carregada.")?;
        let a = l
            .file
            .config
            .accounts
            .get(index)
            .ok_or("Conta inexistente.")?;
        let credential = account_edit::stored_credential(a);
        // auth_mode + auth_options para prefill. Quando a conta tem oauth,
        // o id de modo é o que combina com o provider: hoje o core só
        // emite "oauth" para fluxos com credencial Cuotai; AWS SSO/Bailian
        // não ficam em `oauth` (eles não têm `Account::oauth`).
        let (auth_mode, auth_options, client_id, client_secret_env, tenant) = match &a.oauth {
            Some(cfg) => (
                login::MODE_OAUTH,
                cfg.options.clone(),
                cfg.client_id.clone(),
                cfg.client_secret_env.clone(),
                cfg.tenant.clone(),
            ),
            None => ("credential", BTreeMap::new(), String::new(), None, None),
        };
        let fields = field_descriptors(a.provider)
            .iter()
            .map(|d| match d.kind {
                FieldDescriptorKind::Text => FieldValue {
                    key: d.key.into(),
                    text: Some(a.params.get(d.key).cloned().unwrap_or_default()),
                    choice: None,
                },
                FieldDescriptorKind::Choice(c) => FieldValue {
                    key: d.key.into(),
                    text: None,
                    choice: Some(
                        a.params
                            .get(d.key)
                            .and_then(|v| c.options.iter().position(|(val, _)| val == v))
                            .unwrap_or(0),
                    ),
                },
            })
            .collect();
        Ok(AccountForm {
            name: a.name.clone(),
            provider: a.provider,
            credential,
            interval: a.refresh_seconds.map(|s| s.to_string()).unwrap_or_default(),
            uses_oauth: a.oauth.is_some(),
            native_login: a.oauth.is_some()
                && !requires_client_registration(a.provider)
                && supports_login(a.provider),
            auth_mode,
            auth_options,
            client_id,
            client_secret_env,
            tenant,
            fields,
        })
    }

    // ---------- login (RF2: orquestração no core; aqui só estado e repasse) ----------

    /// Inicia um fluxo de login do core e repassa os eventos tipados ao
    /// frontend via `LOGIN_EVENT`. O driver e a credencial completada ficam
    /// no backend; a UI só recebe `LoginEvent` (sem segredos). Retorna o
    /// `attempt` (monotônico) que o frontend ecoa em `save_account` para
    /// garantir que o pending certo seja consumido.
    pub async fn login_start(
        self: &Arc<Self>,
        app: &AppHandle,
        input: LoginStartInput,
    ) -> Result<u64, String> {
        self.ensure_loaded().await;
        // Sessão, pré-condição e instalação do `PendingLogin` acontecem
        // sob o MESMO lock: evita a janela em que um login_start cancelaria
        // um predecessor e instalaria o novo entre dois `lock().await`,
        // janela na qual um `login_submit_code` ou `save_account` correria
        // contra um slot inexistente e um forwarder velho poderia mutar o
        // novo pendente. O `start_login` do core é síncrono, então a seção
        // crítica é rápida.
        let mut g = self.inner.lock().await;
        if g.login.as_ref().is_some_and(|p| !p.done) {
            return Err("Já há um login em andamento; conclua ou cancele.".into());
        }
        let session = Arc::clone(g.session.as_ref().ok_or("Sessão OAuth não inicializada.")?);
        // RF2.8: reautenticação reaproveita o credential_id da conta
        // existente (mesmo provider); conta nova recebe id novo (None =>
        // o core gera um uuid v4). Nenhuma decisão de fluxo por provider
        // acontece aqui (RF2.6).
        let credential_id = input
            .index
            .and_then(|i| {
                g.loaded
                    .as_ref()
                    .and_then(|l| l.file.config.accounts.get(i))
            })
            .filter(|a| a.provider == input.provider)
            .and_then(|a| a.oauth.as_ref())
            .map(|o| o.credential_id.clone());
        // Valida o NOVO pedido ANTES de tomar o predecessor: se o core
        // recusar (modo não oferecido, indisponível, etc.), a credencial
        // completada do login anterior permanece em memória e pode ser
        // consumida por um `save_account` subsequente.
        let driver = login::start_login(
            session,
            input.provider,
            LoginRequest {
                mode: input.mode.clone(),
                options: input.options.clone(),
                client_id: input.client_id.clone(),
                client_secret_env: input.client_secret_env.clone(),
                tenant: input.tenant.clone(),
                credential_id,
            },
        )
        .map_err(|e| e.to_string())?;
        // Novo pedido validado: agora sim descarta o predecessor terminal
        // (o forwarder dele é cancelado para que não confunda o novo slot).
        if let Some(mut old) = g.login.take() {
            if let Some(cancel) = old.cancel_tx.take() {
                let _ = cancel.send(());
            }
        }
        // Fila de capacidade 1: um código por vez aguardando repasse ao
        // driver; o veredito imediato do core volta pelo oneshot (a
        // validação em si chega depois via LoginEvent).
        let (code_tx, mut code_rx) =
            mpsc::channel::<(String, oneshot::Sender<Result<(), String>>)>(1);
        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
        let attempt = g.next_login_attempt;
        g.next_login_attempt = g.next_login_attempt.wrapping_add(1);
        g.login = Some(PendingLogin {
            attempt,
            provider: input.provider,
            index: input.index,
            code_tx,
            cancel_tx: Some(cancel_tx),
            done: false,
            completed: None,
        });
        let backend = Arc::clone(self);
        let app = app.clone();
        let mut driver = driver;
        let mut cancel_rx = cancel_rx;
        tauri::async_runtime::spawn(async move {
            loop {
                let event = tokio::select! {
                    // Cancelar derruba o driver aqui (Drop aborta): nenhum
                    // byte chega ao cofre antes do commit (RF2.4).
                    _ = &mut cancel_rx => break,
                    code = code_rx.recv() => {
                        match code {
                            Some((code, reply)) => {
                                // Repassa o veredito imediato do core:
                                // Full/Closed/Unsupported voltam à UI com o
                                // código intacto no campo; a validação do
                                // código aceito chega via LoginEvent.
                                let result = match driver.submit_code(code) {
                                    Ok(()) => Ok(()),
                                    Err(login::SubmitCodeError::Unsupported) => Err(
                                        "Este fluxo não aceita código manual.".to_string(),
                                    ),
                                    Err(login::SubmitCodeError::Full(_)) => Err(
                                        "Aguarde a validação do código anterior antes de reenviar."
                                            .to_string(),
                                    ),
                                    Err(login::SubmitCodeError::Closed) => Err(
                                        "A sessão de login foi encerrada.".to_string(),
                                    ),
                                };
                                let _ = reply.send(result);
                                continue;
                            }
                            // Todos os remetentes sumiram (cancel/limpeza):
                            // tratar como cancelamento.
                            None => break,
                        }
                    }
                    event = driver.next_event() => match event {
                        Some(event) => event,
                        None => break,
                    },
                };
                let terminal = matches!(
                    event,
                    LoginEvent::Finished { .. } | LoginEvent::Failed { .. }
                );
                if matches!(event, LoginEvent::Finished { .. }) {
                    // Após Finished, `finish` retorna imediatamente com o
                    // resultado; credencial completada fica no backend.
                    if let Ok(login::LoginResult::Authorized(completed)) = driver.finish().await {
                        let mut g = backend.inner.lock().await;
                        // Guard contra forwarder velho: só muta o slot se
                        // a tentativa ainda for a sua.
                        if let Some(p) = g.login.as_mut() {
                            if p.attempt == attempt {
                                p.completed = Some(completed);
                            }
                        }
                    }
                }
                // Marca `done` e invalida `cancel_tx` ANTES de emitir o
                // evento terminal: o frontend pode chamar `saveAccount`
                // imediatamente após receber o `Finished` e a checagem
                // `p.done` precisa passar já na primeira tentativa.
                if terminal {
                    let mut g = backend.inner.lock().await;
                    if let Some(p) = g.login.as_mut() {
                        if p.attempt == attempt {
                            p.done = true;
                            p.cancel_tx = None;
                        }
                    }
                }
                // Carrega o `attempt` da tentativa atual: o frontend
                // descarta envelopes cuja tentativa não bate com a ativa.
                let attempt_now = {
                    let g = backend.inner.lock().await;
                    g.login
                        .as_ref()
                        .filter(|p| p.attempt == attempt)
                        .map(|p| p.attempt)
                        .unwrap_or(attempt)
                };
                let _ = app.emit(
                    LOGIN_EVENT,
                    LoginEnvelope {
                        attempt: attempt_now,
                        event: &event,
                    },
                );
                if terminal {
                    break;
                }
            }
            // Saída por cancel ou canal fechado: marca `done` para o
            // caminho de submit/cancel saber que o slot expirou.
            let mut g = backend.inner.lock().await;
            if let Some(p) = g.login.as_mut() {
                if p.attempt == attempt {
                    p.done = true;
                    p.cancel_tx = None;
                }
            }
        });
        Ok(attempt)
    }

    /// Envia um código colado manualmente (fluxo MiMo). O veredito imediato
    /// do core (aceito para validação / fila cheia / encerrado) volta ao
    /// chamador; a validação em si chega à UI via `LoginEvent`.
    pub async fn login_submit_code(&self, code: String) -> Result<(), String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        {
            let g = self.inner.lock().await;
            let p = g.login.as_ref().ok_or("Nenhum login em andamento.")?;
            if p.done {
                return Err("O login já terminou; inicie novamente se necessário.".into());
            }
            p.code_tx.try_send((code, reply_tx)).map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => {
                    "Aguarde a validação do código anterior antes de reenviar.".to_string()
                }
                mpsc::error::TrySendError::Closed(_) => {
                    "A sessão de login foi encerrada.".to_string()
                }
            })?;
        }
        // A espera deve terminar com o veredito IMEDIATO do core (Full/etc.)
        // OU com a queda do forwarder (canal oneshot fechado). A janela entre
        // a última ação do forwarder e a marcação de `done` é coberta pelo
        // timeout: a sessão ativa responde em < 1 ms, então 5 s só dispara
        // mesmo se o forwarder morrer antes de responder.
        match tokio::time::timeout(std::time::Duration::from_secs(5), reply_rx).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(message))) => Err(message),
            Ok(Err(_)) => Err("A sessão de login foi encerrada.".to_string()),
            Err(_) => Err("A sessão de login não respondeu.".to_string()),
        }
    }

    /// Cancela o login pendente. Cancelamento antes do commit não grava
    /// nada no cofre; uma credencial completada morre em memória. Quando o
    /// frontend já conhecia um `attempt` específico (passado em
    /// `attempt`), só cancela se o slot ainda for aquele — evita limpar um
    /// login novo que já tomou o lugar do anterior.
    pub async fn login_cancel(&self, attempt: Option<u64>) {
        let mut g = self.inner.lock().await;
        let matches = match (attempt, g.login.as_ref()) {
            (Some(a), Some(p)) => p.attempt == a,
            (None, _) => true,
            (Some(_), None) => false,
        };
        if !matches {
            return;
        }
        if let Some(mut p) = g.login.take() {
            if let Some(cancel) = p.cancel_tx.take() {
                let _ = cancel.send(());
            }
        }
    }
}

// ---------- formulário ----------

#[derive(Deserialize, Serialize)]
pub struct FieldValue {
    pub key: String,
    pub text: Option<String>,
    pub choice: Option<usize>,
}

/// A interface envia os campos em camelCase (`commitLogin`, `loginAttempt`, `authMode`, `cliProfileOverride`).
/// Sem o `rename_all`, o serde ignora o que não conhece e o login concluído nunca chega ao salvar.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInput {
    pub index: Option<usize>,
    pub provider: Provider,
    pub name: String,
    pub credential: String,
    pub fields: Vec<FieldValue>,
    pub interval: String,
    /// RF2: gravar junto a credencial do login pendente desta conta
    /// (cofre + TOML encenados pelo core). `false` = só TOML.
    #[serde(default)]
    pub commit_login: bool,
    /// Identificador da tentativa de login casada com este salvamento.
    /// O `save_account` exige que o `PendingLogin` atual tenha o mesmo
    /// `attempt` — protege contra um forwarder velho/confuso e contra
    /// a UI reaproveitar credenciais de uma tentativa anterior.
    #[serde(default)]
    pub login_attempt: u64,
    /// Modo de autenticação escolhido no diálogo: "credential" para
    /// credencial manual (env/perfil), ou o id de um `LoginMode` do core
    /// (oauth, aws_sso, bailian_console). Determina se o oauth existente
    /// é preservado ou removido.
    #[serde(default = "default_credential_mode")]
    pub auth_mode: String,
    /// Override do `params.cli_profile` (modo `bailian_console`): vazio
    /// deixa o core gerar um identificador único; não-vazio é validado
    /// por `is_valid_cli_profile`.
    #[serde(default)]
    pub cli_profile_override: String,
}

fn default_credential_mode() -> String {
    "credential".to_string()
}

/// Entrada de `login_start`: modo escolhido + opções não secretas. A
/// seleção do fluxo por provider fica no core (RF2.6); aqui só há repasse.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStartInput {
    pub provider: Provider,
    /// Conta em reautenticação (reuso do `credential_id`, RF2.8).
    pub index: Option<usize>,
    /// `id` do modo devolvido por `form_schema` (enviado de volta intacto).
    pub mode: String,
    /// Opções não secretas do fluxo (region/auth_method/start_url,
    /// profile do AWS SSO, console_site+profile do Bailian).
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    /// Registro próprio (modos com `requires_client_registration`).
    pub client_id: Option<String>,
    pub client_secret_env: Option<String>,
    pub tenant: Option<String>,
}

#[derive(Serialize)]
pub struct AccountForm {
    name: String,
    provider: Provider,
    credential: String,
    interval: String,
    uses_oauth: bool,
    /// Login nativo: campos de identidade ficam ocultos (regra do core).
    native_login: bool,
    /// Id do modo de login da config existente (oauth/aws_sso/
    /// bailian_console) para prefill da UI na reautenticação.
    auth_mode: &'static str,
    /// Opções não secretas da OAuthConfig existente (region/auth_method/
    /// start_url, profile do AWS SSO, console_site+profile do Bailian)
    /// para prefill.
    auth_options: BTreeMap<String, String>,
    /// client_id registrado na OAuthConfig existente (prefill).
    client_id: String,
    /// Variável do client secret (apenas NOME, nunca valor) para prefill.
    client_secret_env: Option<String>,
    /// Tenant Microsoft configurado, para prefill.
    tenant: Option<String>,
    fields: Vec<FieldValue>,
}

#[derive(Serialize)]
pub struct ProviderInfo {
    id: Provider,
    label: &'static str,
}

pub fn providers() -> Vec<ProviderInfo> {
    Provider::ALL
        .iter()
        .map(|&id| ProviderInfo {
            id,
            label: id.label(),
        })
        .collect()
}

#[derive(Serialize)]
pub struct ChoiceSchema {
    id: String,
    value: String,
    label: cuotai_core::text::Localized,
}

#[derive(Serialize)]
pub struct FieldSchema {
    id: String,
    key: &'static str,
    label: cuotai_core::text::Localized,
    help: cuotai_core::text::Localized,
    required: bool,
    options: Option<Vec<ChoiceSchema>>,
}

#[derive(Serialize)]
pub struct LoginOptionSchema {
    id: String,
    key: String,
    label: cuotai_core::text::Localized,
    help: cuotai_core::text::Localized,
    choices: Option<Vec<ChoiceSchema>>,
}

#[derive(Serialize)]
pub struct LoginModeSchema {
    id: String,
    label: cuotai_core::text::Localized,
    help: cuotai_core::text::Localized,
    requires_client_registration: bool,
    options: Vec<LoginOptionSchema>,
    unavailable_reason: Option<cuotai_core::text::Localized>,
}

#[derive(Serialize)]
pub struct ProviderLoginSchema {
    modes: Vec<LoginModeSchema>,
    unsupported_reason: Option<cuotai_core::text::Localized>,
}

#[derive(Serialize)]
pub struct FormSchema {
    credential_kind: &'static str,
    default_env: &'static str,
    credential_help: cuotai_core::text::Localized,
    cost_advisory: Option<cuotai_core::text::Localized>,
    fields: Vec<FieldSchema>,
    login: ProviderLoginSchema,
}

pub fn form_schema(provider: Provider) -> FormSchema {
    let slug = provider.slug();
    let login = login::login_modes(provider);
    FormSchema {
        credential_kind: match account_edit::credential_kind(provider) {
            CredentialKind::AwsProfile => "profile",
            CredentialKind::None => "none",
            CredentialKind::EnvVar => "env",
        },
        default_env: default_env(provider),
        credential_help: credential_help(provider),
        cost_advisory: account_edit::cost_advisory(provider),
        login: ProviderLoginSchema {
            modes: login.modes.into_iter().map(|mode| {
                let mode_id = mode.id.clone();
                LoginModeSchema {
                    id: mode.id,
                    label: cuotai_core::text::Localized::fixed(format!("login.{slug}.{mode_id}.label"), mode.label),
                    help: cuotai_core::text::Localized::fixed(format!("login.{slug}.{mode_id}.help"), mode.help),
                    requires_client_registration: mode.requires_client_registration,
                    unavailable_reason: mode.unavailable_reason.map(|fallback| cuotai_core::text::Localized::fixed(
                        format!("login.{slug}.{mode_id}.unavailable_reason"), fallback)),
                    options: mode.options.into_iter().map(|option| {
                        let key = option.key.clone();
                        LoginOptionSchema {
                            id: format!("native.{slug}.{key}"),
                            key: option.key,
                            label: cuotai_core::text::Localized::fixed(
                                account_edit::native_option_label_id(provider, &key), option.label),
                            help: cuotai_core::text::Localized::fixed(
                                account_edit::native_option_help_id(provider, &key), option.help),
                            choices: option.choices.map(|choices| choices.into_iter().map(|(value, fallback)| {
                                let id = account_edit::native_option_option_id(provider, &key, &value);
                                ChoiceSchema { id: id.clone(), value, label: cuotai_core::text::Localized::fixed(id, fallback) }
                            }).collect()),
                        }
                    }).collect(),
                }
            }).collect(),
            unsupported_reason: account_edit::oauth_unsupported_reason(provider),
        },
        fields: field_descriptors(provider).iter().map(|d| FieldSchema {
            id: format!("field.{slug}.{}", d.key),
            key: d.key,
            label: cuotai_core::text::Localized::fixed(account_edit::field_label_id(provider, d.key), d.label),
            help: cuotai_core::text::Localized::fixed(account_edit::field_help_id(provider, d.key), d.help),
            required: d.required,
            options: match d.kind {
                FieldDescriptorKind::Text => None,
                FieldDescriptorKind::Choice(c) => Some(c.options.iter().map(|&(value, fallback)| {
                    let id = account_edit::field_option_id(provider, d.key, value);
                    ChoiceSchema { id: id.clone(), value: value.to_owned(), label: cuotai_core::text::Localized::fixed(id, fallback) }
                }).collect()),
            },
        }).collect(),
    }
}

/// Resultado de remover uma conta, para a UI dizer a verdade sobre o cofre.
#[derive(Serialize)]
pub struct DeleteResult {
    /// "removed" (sem credencial no cofre), "purged" (apagada do cofre), "kept" (mantida a pedido),
    /// "kept_shared" (mantida porque outra conta a usa) ou "partial_failure" (TOML salvo, cofre não apagado).
    outcome: &'static str,
    other_account: Option<String>,
    message: Option<String>,
}

impl DeleteResult {
    fn new(outcome: &'static str) -> Self {
        Self {
            outcome,
            other_account: None,
            message: None,
        }
    }
}

/// Novo ciclo para a configuração `config` mantendo os relatórios do ciclo anterior (sem consultar nada).
/// Os prazos recomeçam a contar a partir de agora com os intervalos efetivos novos.
fn rebuild_cycle_keeping_reports(old: &RefreshCycle, config: &Config) -> RefreshCycle {
    let mut cycle = RefreshCycle::new(config, Instant::now());
    if let Some(snapshot) = old.snapshot().cloned() {
        if snapshot.reports.len() == config.accounts.len() {
            if let Some(token) = cycle.begin_full() {
                cycle.finish_full(Some(snapshot), token, Instant::now());
            }
        }
    }
    cycle
}

/// O modo de login nativo (OAuth) exige uma autorização concluída ou uma credencial já existente para salvar.
/// `aws_sso` e `bailian_console` gravam fora do cofre do Cuotai e não passam por aqui.
fn login_needed_before_save(auth_mode: &str, oauth_mode: bool) -> bool {
    auth_mode == login::MODE_OAUTH && !oauth_mode
}

/// Chaves dos campos visíveis para as escolhas atuais (regra do core). `auth_mode` é o modo escolhido no
/// diálogo ("credential" ou o id de um `LoginMode`): o login nativo esconde os campos de identidade
/// que o próprio login preenche, igual à regra de `build_account` (modo OAuth, sem registro de cliente).
pub fn visible_keys(
    provider: Provider,
    choices: BTreeMap<String, usize>,
    auth_mode: &str,
    login: &BTreeMap<String, String>,
) -> Vec<&'static str> {
    let mut vis = FieldVisibility::new(login, &choices);
    vis.native_login = auth_mode == login::MODE_OAUTH
        && !requires_client_registration(provider)
        && supports_login(provider);
    let d = field_descriptors(provider);
    visible_field_indices(provider, &vis)
        .into_iter()
        .map(|i| d[i].key)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metric(label: &str, period: Option<&str>) -> Metric {
        let mut m = Metric::usage(label, 1.0, "solicitações");
        m.period = period.map(str::to_owned);
        m
    }

    #[test]
    fn global_interval_change_keeps_reports_and_reschedules_deadlines() {
        let mut config = Config::default();
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        let token = cycle.begin_full().expect("rodada completa");
        let reports = config
            .accounts
            .iter()
            .map(|a| Report {
                account: a.name.clone(),
                provider: a.provider,
                checked_at: chrono::Utc::now(),
                status: Status::Ready,
                metrics: vec![],
                notes: vec![],
                notes_l10n: vec![],
                source: "teste".into(),
            })
            .collect();
        let snapshot = cuotai_core::Snapshot {
            started_at: chrono::Utc::now(),
            finished_at: chrono::Utc::now(),
            reports,
        };
        cycle.finish_full(Some(snapshot), token, Instant::now());
        assert_eq!(cycle.snapshot().map(|s| s.reports.len()), Some(config.accounts.len()));

        config.refresh_seconds = 3600;
        let rebuilt = rebuild_cycle_keeping_reports(&cycle, &config);
        // relatórios preservados, nenhuma conta precisa de nova consulta
        assert_eq!(rebuilt.snapshot().map(|s| s.reports.len()), Some(config.accounts.len()));
        assert!(rebuilt.pending_indices().is_empty());
        // contas sem intervalo próprio passam a seguir o novo valor global (≈ 1 h)
        let next = rebuilt.next_refresh_in(0, Instant::now());
        assert!(next > Duration::from_secs(3000), "prazo não foi reagendado: {next:?}");
    }

    #[test]
    fn login_start_input_reads_the_camel_case_fields_the_ui_sends() {
        let input: LoginStartInput = serde_json::from_value(serde_json::json!({
            "provider": "gemini_api", "index": null, "mode": "oauth", "options": {},
            "clientId": "meu-client", "clientSecretEnv": "MEU_SEGREDO", "tenant": null
        }))
        .expect("pedido de login");
        assert_eq!(input.client_id.as_deref(), Some("meu-client"), "clientId foi ignorado");
        assert_eq!(input.client_secret_env.as_deref(), Some("MEU_SEGREDO"));
    }

    #[test]
    fn account_input_reads_the_camel_case_fields_the_ui_sends() {
        // Exatamente o objeto que `saveAccount` do diálogo monta ao salvar depois de um login concluído.
        let input: AccountInput = serde_json::from_value(serde_json::json!({
            "index": null, "provider": "kiro", "name": "Kiro Pessoal", "credential": "",
            "fields": [], "interval": "",
            "commitLogin": true, "loginAttempt": 7, "authMode": "oauth", "cliProfileOverride": "perfil"
        }))
        .expect("pedido do diálogo");
        assert!(input.commit_login, "commitLogin foi ignorado");
        assert_eq!(input.login_attempt, 7);
        assert_eq!(input.auth_mode, "oauth");
        assert_eq!(input.cli_profile_override, "perfil");
    }

    #[test]
    fn oauth_mode_without_login_is_reported_as_missing_login_not_missing_env_var() {
        assert!(login_needed_before_save(login::MODE_OAUTH, false));
        assert!(!login_needed_before_save(login::MODE_OAUTH, true)); // login concluído ou credencial existente
        assert!(!login_needed_before_save("credential", false)); // credencial manual: o core cobra a variável
        assert!(!login_needed_before_save(login::MODE_AWS_SSO, false));
        assert!(!login_needed_before_save(login::MODE_BAILIAN_CONSOLE, false));
    }

    #[test]
    fn native_login_hides_the_identity_fields_the_login_fills_in() {
        let none = BTreeMap::new();
        let keys = |mode: &str| visible_keys(Provider::Kiro, BTreeMap::new(), mode, &none);
        // credencial manual: o método de autenticação (origem do token) é um campo editável
        assert!(keys("credential").contains(&"auth_method"));
        // login no navegador: o próprio login define o método; o campo não pode aparecer duplicado
        let native = keys(login::MODE_OAUTH);
        assert!(!native.contains(&"auth_method"));
        // o ARN continua editável enquanto o login não o fixar
        assert!(native.contains(&"profile_arn"));
    }

    #[test]
    fn recognizes_five_hour_and_weekly_windows_from_adapter_labels() {
        let w = |label: &str, period: Option<&str>| metric_window(&metric(label, period));
        for five in [
            "Sessão (5 horas) (%)",
            "Codex — janela de 5 hora(s) (%)",
            "M-series (5h)",
            "Coding Plan: 5h",
            "Gemini (5h)",
            "session (5h)",
            "Claude & GPT (5h)",
        ] {
            assert_eq!(w(five, None), Some("5h"), "{five}");
        }
        for week in [
            "Semana (7 dias) (%)",
            "Semanal",
            "weekly (7d)",
            "Grok CLI: semanal SuperGrok",
            "Gemini (Semanal)",
            "M-series (7d)",
        ] {
            assert_eq!(w(week, None), Some("week"), "{week}");
        }
        for other in ["Monthly", "credit", "Plan (USD)", "Cursor plano (inclusão)", "Balance", "25h extra"] {
            assert_eq!(w(other, None), None, "{other}");
        }
        // rate limit nunca é janela de cota, mesmo dizendo "5h" no período
        assert_eq!(w("Rate limit", Some("janela de 5h")), None);
        assert_eq!(w("Ratelimit", Some("semanal")), None);
        assert_eq!(w("Tokens", Some("janela corrente do rate limit")), None);
    }

    #[test]
    fn rate_limit_windows_are_not_plan_quotas() {
        assert_eq!(metric_kind(&metric("Rate limit", None)), "rate_limit");
        assert_eq!(metric_kind(&metric("Ratelimit", None)), "rate_limit");
        assert_eq!(
            metric_kind(&metric("Tokens", Some("janela corrente do rate limit"))),
            "rate_limit"
        );
        assert_eq!(metric_kind(&metric("Semanal", Some("semanal"))), "quota");
        assert_eq!(metric_kind(&metric("Monthly", None)), "quota");
    }

    #[test]
    fn first_sweep_does_not_wait_for_the_periodic_deadline() {
        // Deadlines de um ciclo recém-criado estão no futuro (agora + intervalo).
        let config = Config::default();
        let cycle = RefreshCycle::new(&config, Instant::now());
        assert!(cycle.pending_now_indices(Instant::now()).is_empty());
        let tokens = vec![None; config.accounts.len()];
        assert!(needs_initial_sweep(&cycle, &tokens));
    }

    #[test]
    fn no_sweep_while_a_slot_is_busy() {
        let config = Config::default();
        let cycle = RefreshCycle::new(&config, Instant::now());
        let mut tokens = vec![None; config.accounts.len()];
        tokens[0] = cycle_token();
        assert!(!needs_initial_sweep(&cycle, &tokens));
    }

    fn cycle_token() -> Option<cuotai_core::refresh::Generation> {
        let config = Config::default();
        RefreshCycle::new(&config, Instant::now()).begin(0)
    }
}
