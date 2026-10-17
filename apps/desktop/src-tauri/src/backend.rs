//! Ponte entre `cuotai-core` e a UI. Não contém regra de negócio: validação,
//! edição de contas, escrita do TOML, classificação de métricas e agendamento
//! vêm do core; aqui só há estado, DTOs e orquestração.
use cuotai_core::{
    account_edit::{
        self, build_account, credential_help, default_env, field_descriptors,
        visible_field_indices, AccountDraft, CredentialKind, DraftField, DraftValue,
        FieldDescriptorKind, FieldVisibility,
    },
    config_file::{ConfigFile, Discovery, Locations, PathSource, ResolvedPath},
    identity::{account_identity, AccountIdentity},
    login::{self, CompletedLogin, LoginEvent, LoginRequest},
    metrics::{classify_metric, classify_report, AccountClass, MetricState},
    models::{self, ModelError, ModelState},
    oauth::OAuthSession,
    refresh::{Edit, RefreshCycle},
    Config, EnvironmentCredentials, Metric, Monitor, Provider, Report, Status,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    future::Future,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::{async_runtime::Mutex, AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};

fn l10n_to_json(note: &cuotai_core::text::Localized) -> String {
    serde_json::to_string(note).unwrap_or_else(|_| note.fallback.to_string())
}

/// Encapsulate an `EditError` for the frontend: the core's RF8
/// `Localized` plus an optional field token the dialog uses to
/// highlight the offending row (`name`, `credential`, `interval`,
/// `params.<key>`, etc.). JSON shape:
///
/// `{ id, params, fallback, field? }`
///
/// `field` is omitted when the error is document-wide (no specific
/// input) or when the variant isn't a `Validation` (e.g. `Conflict`,
/// `Io`, `Unsupported`). Returned as a JSON string so it can cross
/// the Tauri boundary the same way the rest of the errors already
/// do — the frontend parses it in `account-dialog.js`'s `errText`.
#[derive(serde::Serialize)]
struct EditErrorEnvelope<'a> {
    #[serde(flatten)]
    note: &'a cuotai_core::text::Localized,
    #[serde(skip_serializing_if = "Option::is_none")]
    field: Option<&'static str>,
}

fn edit_error_to_json(error: &cuotai_core::edit_error::EditError) -> String {
    let envelope = EditErrorEnvelope {
        note: &error.l10n(),
        field: error.field(),
    };
    serde_json::to_string(&envelope).unwrap_or_else(|_| error.message())
}

fn refresh_edit_error(error: &cuotai_core::refresh::EditError) -> cuotai_core::text::Localized {
    match error {
        cuotai_core::refresh::EditError::Index(index) => cuotai_core::text::Localized::with_param(
            "edit_error.index_out_of_range",
            "index",
            *index,
            error.to_string(),
        ),
        cuotai_core::refresh::EditError::UnexpectedDiff => {
            cuotai_core::text::Localized::fixed("edit_error.refresh_cycle_diff", error.to_string())
        }
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
    /// RF3 retry handles: `RemovedAccount`s retained after a removal
    /// that left the credential in the vault (`CredentialKept`) or
    /// whose vault delete failed (`PartialFailure`). The vault key
    /// changes when `install` rebuilds the live configuration, so we
    /// drop the entire list on reload/replacement to honour the
    /// invariant "a retry must not delete a credential now referenced
    /// again". The core guard still runs on every retry against the
    /// current config; the handle list is the UX primitive that lets
    /// the desktop offer a "Tentar de novo" button without depending
    /// on the removed account index (which no longer exists in TOML).
    pending_recovery: Vec<RecoveryEntry>,
}

/// RF3 recovery primitive stored in the desktop so the UI can retry
/// a vault purge against the *current* configuration. `handle` is an
/// opaque id the UI round-trips; `account` carries the
/// non-secret [`account_edit::RemovedAccount`] the core needs to
/// re-derive the vault key and run the shared-key guard.
#[derive(Debug, Clone)]
struct RecoveryEntry {
    handle: String,
    account: account_edit::RemovedAccount,
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
    /// Core classification, flattened: `state` ("unmeasured" | "exhausted" |
    /// "available") plus `no_cap` or `ratio`/`uncapped`.
    #[serde(flatten)]
    state: MetricState,
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
    class: AccountClass,
    refresh_seconds: u64,
    /// Instante (Unix, ms) da próxima consulta agendada desta conta; `None` enquanto ela está sendo consultada.
    next_refresh_at_ms: Option<u64>,
    uses_oauth: bool,
    identity: AccountIdentity,
}

#[derive(Serialize, Clone)]
pub struct View {
    /// Identidade normalizada (symlinks/relativos resolvidos) do arquivo em uso.
    config_path: Option<String>,
    /// Regra que escolheu o arquivo: `env`, `chosen` ou `system`.
    config_source: Option<PathSource>,
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
    /// RF6.1: identidade da configuração carregada (muda a cada save/remove/move/reload/troca);
    /// a UI a devolve nos comandos de modelos. 0 antes de carregar (nunca aceita).
    generation: u64,
}

fn metric_view(metric: &Metric) -> MetricView {
    MetricView {
        metric: metric.clone(),
        state: classify_metric(metric),
    }
}

fn report_view(
    index: usize,
    report: &Report,
    config: &Config,
    next_refresh_at_ms: Option<u64>,
) -> ReportView {
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
        class: classify_report(Some(report)),
        refresh_seconds: account.effective_refresh_seconds(config.refresh_seconds),
        next_refresh_at_ms,
        uses_oauth: account.oauth.is_some(),
        identity: account_identity(account, report.status),
    }
}

impl Inner {
    fn view(&self) -> View {
        let Some(l) = &self.loaded else {
            return View {
                config_path: None,
                config_source: None,
                config_exists: false,
                account_count: 0,
                loaded: false,
                started_at: None,
                finished_at: None,
                reports: vec![],
                busy: vec![],
                error: self.load_error.clone(),
                refresh_seconds: 0,
                generation: 0,
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
                slot.as_ref().map(|report| {
                    // Próxima consulta como instante absoluto: a UI faz a contagem regressiva sem novos eventos.
                    let next = (!l.cycle.is_busy(i))
                        .then(|| unix_ms + l.cycle.next_refresh_in(i, now).as_millis() as u64);
                    report_view(i, report, &l.file.config, next)
                })
            })
            .collect();
        View {
            config_path: Some(config_label(&l.file)),
            config_source: Some(l.file.source),
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
            generation: l.generation,
        }
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
        let monitor = Monitor::with_oauth(
            file.config.clone(),
            Arc::new(EnvironmentCredentials),
            session,
        )
        .map_err(|e| {
            serde_json::to_string(&cuotai_core::edit_error::config_error_note(&e)).unwrap_or_else(
                |_| {
                    cuotai_core::edit_error::config_error_note(&e)
                        .fallback
                        .into_owned()
                },
            )
        })?;
        // Keyed by `ConfigFile::identity()` in the OS data dir; no data dir
        // leaves model operations as `NoCache` while quotas keep working.
        let monitor = Arc::new(match cuotai_core::models::ModelCache::in_data_dir(&file) {
            Ok(cache) => monitor.with_model_cache(cache),
            Err(_) => monitor,
        });
        // Just loaded from the TOML: drop catalogs of accounts gone from it.
        // A cache failure only keeps stale digests; model ops report it.
        let _ = monitor.prune_models();
        self.generation += 1;
        let accounts_len = file.config.accounts.len();
        // `install` replaces the live configuration (initial load or
        // a `cuotai.toml` reload after `Conflict`).
        // Recovery handles minted under the previous configuration
        // reference vault entries that may now be re-used by a fresh
        // account in the new config; the core guard already refuses
        // such retries, but dropping the list here means the UI will
        // not offer a button for a stale handle in the first place.
        self.loaded = Some(Loaded {
            cycle: RefreshCycle::new(&file.config, Instant::now()),
            file,
            monitor,
            generation: self.generation,
            tokens: vec![None; accounts_len],
            pending_recovery: Vec::new(),
        });
        self.load_error = None;
        Ok(())
    }
}

fn config_label(file: &ConfigFile) -> String {
    file.identity()
        .map(|id| id.to_string())
        .unwrap_or_else(|_| file.path.display().to_string())
}

impl Inner {
    /// RF1.2: `CUOTAI_CONFIG` > arquivo escolhido nas configurações > caminho
    /// do SO (nunca `./cuotai.toml` do `cwd`). Carrega via core e troca todo o
    /// estado por configuração (monitor, agenda, tokens, recuperação); em erro
    /// o estado atual fica intacto.
    fn switch(&mut self, chosen: Option<PathBuf>, locations: &Locations) -> Result<(), String> {
        let file = ConfigFile::load_with(Discovery::Gui { chosen }, true, locations)
            .map_err(|e| edit_error_to_json(&e))?;
        self.install(file)
    }
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
        let chosen = crate::settings::chosen_config();
        if let Err(e) = g.switch(chosen, &Locations::from_process()) {
            g.load_error = Some(e);
        }
    }

    /// Aplica o `cuotai.toml` escolhido nas configurações (`None` = padrão do
    /// SO): recarrega pelo core, limpa o estado da configuração anterior e só
    /// então persiste a escolha em `desktop.json`.
    pub async fn set_config_file(
        &self,
        app: &AppHandle,
        path: Option<PathBuf>,
    ) -> Result<View, String> {
        let locations = Locations::from_process();
        if locations.env.is_some() {
            return Err("env_locked".into());
        }
        if let Some(p) = &path {
            crate::settings::validate_config_file(p).map_err(str::to_owned)?;
        }
        let mut g = self.inner.lock().await;
        g.switch(path.clone(), &locations)?;
        crate::settings::save_config_file(path.as_deref()).map_err(str::to_owned)?;
        let view = g.view();
        drop(g);
        self.emit(app).await;
        Ok(view)
    }

    pub async fn view(&self) -> View {
        self.ensure_loaded().await;
        self.inner.lock().await.view()
    }

    /// RF4 reload path: re-read the config from disk and replace the
    /// live state with the freshly-loaded one. Used after an
    /// `EditError::Conflict` (`save`/`delete`/`move`/`set_refresh_seconds`
    /// refused to write because the in-memory snapshot diverged from
    /// disk) and as the cheap way to pick up external edits detected
    /// via [`Backend::config_changed_on_disk`].
    ///
    /// - same path and source as the live file (a later change of
    ///   `CUOTAI_CONFIG` or of the chosen file cannot silently switch
    ///   files mid-session);
    /// - aborts/clears in-flight refreshes of the previous generation
    ///   by rebuilding monitor/cycle/tokens via `install`;
    /// - drops every RF3 recovery handle (the new config may reference
    ///   the same vault entry under a different account);
    /// - on read failure leaves the live state untouched and surfaces
    ///   the typed error (`EditError::Conflict`/`Validation`/`Io`) so
    ///   the UI can keep the dialog draft and the user can decide.
    ///
    /// The new `View` is returned so the frontend can refresh without
    /// a separate round-trip.
    pub async fn reload_config(&self, app: &AppHandle) -> Result<View, String> {
        let mut g = self.inner.lock().await;
        if g.loaded.is_none() {
            // Nothing to reload yet: behave like the first boot so the
            // dialog draft survives (no "Configuração não carregada"
            // bounce back to the user). The view is rebuilt below.
            drop(g);
            self.ensure_loaded().await;
            g = self.inner.lock().await;
        }
        let Some(l) = g.loaded.as_ref() else {
            // The boot load just failed again: surface its typed error.
            return Err(g.load_error.clone().unwrap_or_default());
        };
        let resolved = ResolvedPath {
            path: l.file.path.clone(),
            source: l.file.source,
        };
        let file = ConfigFile::open(resolved, true).map_err(|e| edit_error_to_json(&e))?;
        // Carry session + load_error reset through `install` (clears
        // them on success, retains the inner state on failure). After
        // a successful `install` the cycle is fresh, the monitor is
        // new and in-flight refreshes of the old generation are
        // rejected by their tokens.
        g.install(file)?;
        let view = g.view();
        drop(g);
        self.emit(app).await;
        Ok(view)
    }

    /// Cheap detector: did the file on disk change since we last
    /// loaded or saved it? Returns `false` when no config is loaded
    /// (the very first sweep races the load) and surfaces an `Io`
    /// error so the UI can keep the dialog open.
    ///
    /// Implemented by comparing the current bytes to the snapshot
    /// the `ConfigFile` already holds (see `ConfigFile::changed_on_disk`),
    /// so a rewrite that keeps the mtime identical (some filesystems
    /// have second-level resolution) is still detected.
    pub async fn config_changed_on_disk(&self) -> Result<bool, String> {
        let g = self.inner.lock().await;
        let Some(l) = g.loaded.as_ref() else {
            return Ok(false);
        };
        l.file.changed_on_disk().map_err(|e| edit_error_to_json(&e))
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

    pub async fn save_account(
        &self,
        app: &AppHandle,
        input: AccountInput,
    ) -> Result<SaveAccountResult, String> {
        self.ensure_loaded().await;
        // Everything below runs under ONE inner.lock so a concurrent
        // refresh / scheduler tick cannot observe a half-applied edit.
        // The commit path holds the lock across `commit_login`'s vault
        // write (spawn_blocking against the local vault, brief): the
        // desktop is single-user with a modal dialog, and keeping the
        // lock avoids any half-applied window between the vault and
        // TOML stages.
        let (refresh_target, cleanup_warning): (
            Option<usize>,
            Option<cuotai_core::text::Localized>,
        ) = {
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
            let selected_mode = if input.auth_mode == "credential" {
                None
            } else {
                Some(
                    login::login_mode(input.provider, &input.auth_mode)
                        .ok_or("Modo de autenticação não oferecido para este provedor.")?,
                )
            };
            let vault_mode = selected_mode.as_ref().is_some_and(|mode| {
                mode.credential_store == login::LoginCredentialStore::CuotaiVault
            });
            let oauth_existing = if vault_mode && had_oauth {
                existing
                    .filter(|a| a.provider == input.provider)
                    .and_then(|a| a.oauth.as_ref())
                    .cloned()
            } else {
                None
            };
            let oauth_mode = vault_mode && (completed.is_some() || oauth_existing.is_some());
            if login_needed_before_save(selected_mode.as_ref(), oauth_mode) {
                return Err(
                    "Conclua o login antes de salvar (ou volte para a credencial manual).".into(),
                );
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
            let oauth_login_params = completed.map(|c| c.params.clone()).unwrap_or_default();
            let (credential, credential_params) = if let Some(mode) = &selected_mode {
                login::account_login_inputs(
                    input.provider,
                    &mode.id,
                    &input.credential,
                    &input.auth_options,
                    &input.auth_params,
                    input.commit_login,
                )
                .map_err(|error| error.to_string())?
            } else {
                (input.credential.clone(), None)
            };
            let draft = AccountDraft {
                provider: input.provider,
                name: &input.name,
                credential: &credential,
                auth_mode: &input.auth_mode,
                oauth_mode,
                oauth_existing: oauth_existing.as_ref(),
                oauth_authorized: completed.map(|c| c.config.clone()),
                oauth_login_params,
                existing_params: None,
                credential_params,
                fields: &fields,
                interval_text: &input.interval,
            };
            let account =
                build_account(&draft, config, input.index).map_err(|e| l10n_to_json(&e.l10n()))?;
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
                    return Err(l10n_to_json(&cuotai_core::edit_error::config_error_note(
                        &e,
                    )));
                }
            };
            // Persist. Core owns vault→TOML ordering, compensation and the
            // post-save replaced-key cleanup/shared-key guard.
            let mut cleanup_warning = None;
            if let Some(completed) = completed {
                let session = session
                    .as_ref()
                    .map(Arc::as_ref)
                    .ok_or("Sessão OAuth não inicializada.")?;
                match login::commit_login(&mut l.file, session, completed, updated.clone()).await {
                    login::LoginCommitOutcome::Saved => {}
                    login::LoginCommitOutcome::SavedWithCleanupFailure(failure) => {
                        cleanup_warning = Some(cuotai_core::text::Localized::fixed(
                            "edit_error.login_cleanup_failed",
                            failure.message,
                        ));
                    }
                    login::LoginCommitOutcome::PartialFailure(failure) => {
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
                return Err(edit_error_to_json(&e));
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
            // Saved: an edited identity (rename, credential, region) loses its entry.
            let _ = l.monitor.prune_models();
            if l.tokens.len() < l.file.config.accounts.len() {
                l.tokens
                    .extend((0..l.file.config.accounts.len() - l.tokens.len()).map(|_| None));
            }
            if let Some(slot) = l.tokens.get_mut(edit_index) {
                *slot = None;
            }
            l.generation += 1;
            *generation = l.generation;
            (refresh_target, cleanup_warning)
        };
        self.emit(app).await;
        if let Some(index) = refresh_target {
            self.refresh_account(app, index).await?;
        }
        Ok(SaveAccountResult { cleanup_warning })
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
        let result = self.drive_delete_account(index, purge_credentials).await?;
        self.emit(app).await;
        Ok(result)
    }

    /// Testable inner half of [`Backend::delete_account`]: stage the
    /// removal under the inner mutex (snapshot session, validate the
    /// index, write the TOML, mirror cycle + monitor + tokens +
    /// generation), release the mutex, then run the optional vault
    /// purge without holding it. Splitting the work from `emit` lets
    /// the unit tests drive the state machine without standing up a
    /// Tauri runtime.
    pub(crate) async fn drive_delete_account(
        &self,
        index: usize,
        purge_credentials: bool,
    ) -> Result<DeleteResult, String> {
        // RF3 split: stage the removal under the inner mutex (snapshot
        // session, validate the index, write the TOML, mirror cycle +
        // monitor + tokens + generation), release the mutex, then run
        // the optional vault purge without holding it. The desktop UI
        // sends `purge_credentials = true` whenever the user picks
        // "Remover e apagar do cofre" (`dialog.deleteVaultPurge`), so
        // the vault IPC round-trip can take a moment; releasing the
        // mutex here unblocks `view`, `refresh_*` and `login_*` while
        // we wait for the OS keychain.
        let staged = {
            let mut g = self.inner.lock().await;
            let session = match &g.session {
                Some(s) => Arc::clone(s),
                None => return Err("Sessão OAuth não inicializada.".into()),
            };
            let l = g.loaded.as_mut().ok_or("Configuração não carregada.")?;
            let prev = l.file.config.clone();
            if index >= prev.accounts.len() {
                return Err("Conta inexistente.".into());
            }
            // Pre-compute the new config + cycle + monitor so a
            // failure (cycle refusal, monitor refusal, save failure)
            // short-circuits without ever touching the vault.
            let updated = prev
                .without_account(index)
                .map_err(|e| edit_error_to_json(&e))?;
            let mut staged_cycle = l.cycle.clone_state_for_edit();
            staged_cycle
                .apply_edit(&prev, &updated, Edit::Remove(index), Instant::now())
                .map_err(|e| l10n_to_json(&refresh_edit_error(&e)))?;
            let new_monitor = l
                .monitor
                .with_config(updated.clone())
                .map_err(|e| l10n_to_json(&cuotai_core::edit_error::config_error_note(&e)))?;
            // Sync TOML save via the core; the core still owns the
            // shared-key guard and the partial-failure invariant.
            // On any error here `l.file` is unchanged and the staged
            // cycle + monitor are dropped on return.
            let staged =
                account_edit::remove_account_save_only(&mut l.file, index, purge_credentials)
                    .map_err(|error| l10n_to_json(&account_edit::RemoveError::l10n(&error)))?;
            // Mirror the staged runtime into the live state. We
            // snapshot the post-save config BEFORE assigning
            // `g.generation` so the borrows do not overlap
            // (Rust's NLL tracks the lifetime of the cloned value).
            let post_save_config = l.file.config.clone();
            l.cycle = staged_cycle;
            l.monitor = Arc::new(new_monitor);
            let _ = l.monitor.prune_models();
            if index < l.tokens.len() {
                l.tokens.remove(index);
            }
            l.generation += 1;
            g.generation = l.generation;
            (staged, session, post_save_config)
        };
        // We are OUT of the inner mutex from here. The vault purge,
        // when applicable, runs without blocking other backend
        // commands.
        let (staged, session, post_save_config) = staged;
        let (result, pending_recovery) = match staged {
            account_edit::StagedRemoval::RemovedNoCredential(_) => {
                (DeleteResult::new("removed"), None)
            }
            account_edit::StagedRemoval::Kept { account, reason } => {
                let handle = mint_handle();
                let result = match reason {
                    account_edit::KeptReason::Declined => DeleteResult::new("kept"),
                    account_edit::KeptReason::SharedWith { other_account } => DeleteResult {
                        outcome: "kept_shared",
                        other_account: Some(other_account),
                        message: None,
                        recovery_handle: None,
                    },
                };
                (
                    result.with_recovery(handle.clone()),
                    Some((handle, account)),
                )
            }
            account_edit::StagedRemoval::ReadyForPurge {
                account,
                provider,
                oauth,
            } => match account_edit::purge_credential(
                session.as_ref(),
                &post_save_config,
                provider,
                &oauth,
            )
            .await
            {
                Ok(()) => (DeleteResult::new("purged"), None),
                Err(vault_error) => {
                    // Mint a recovery handle so the UI can offer a
                    // retry. The handle references the
                    // `RemovedAccount` we keep in the backend state
                    // below; the core guard still runs against the
                    // *live* config on every retry.
                    let handle = mint_handle();
                    (
                        DeleteResult {
                            outcome: "partial_failure",
                            other_account: None,
                            message: Some(vault_error.to_string()),
                            recovery_handle: Some(handle.clone()),
                        },
                        Some((handle, account)),
                    )
                }
            },
        };
        // Re-acquire the mutex briefly to register the recovery
        // handle. The handle MUST NOT survive a config replacement
        // (`install` already clears the list); the mutex section here
        // is just bookkeeping.
        if let Some((handle, account)) = pending_recovery {
            let mut g = self.inner.lock().await;
            if let Some(l) = g.loaded.as_mut() {
                l.pending_recovery.push(RecoveryEntry { handle, account });
            }
        }
        Ok(result)
    }

    /// RF3: re-attempt a vault purge against the *current* configuration
    /// using the [`account_edit::RemovedAccount`] the backend
    /// retained when the original removal could not (or would not)
    /// touch the vault. The shared-key guard runs on every call, so a
    /// retry after the survivor was dropped succeeds and a retry
    /// while a new shared reference exists is refused with the same
    /// honest message the core uses everywhere else. The handle is
    /// consumed on success (no further retry) and left in place on
    /// failure (the UI may retry again).
    pub async fn retry_purge(
        &self,
        app: &AppHandle,
        handle: String,
    ) -> Result<DeleteResult, String> {
        self.ensure_loaded().await;
        let result = self.drive_retry(&handle).await?;
        self.emit(app).await;
        Ok(result)
    }

    /// Testable inner half of [`Backend::retry_purge`]: snapshots
    /// the recovery state under the mutex, releases it, calls the
    /// core's `purge_credential` outside the lock, then re-acquires
    /// the mutex briefly to drop the handle on success. Splitting
    /// the work from `emit` lets the unit tests drive the state
    /// machine without standing up a Tauri runtime.
    pub(crate) async fn drive_retry(&self, handle: &str) -> Result<DeleteResult, String> {
        // Snapshot the session + the live config + the recovery
        // entry under the mutex, then release it before the vault
        // round-trip (same split as `delete_account`).
        let (session, current_config, removed) = {
            let g = self.inner.lock().await;
            let session = match &g.session {
                Some(s) => Arc::clone(s),
                None => return Err("Sessão OAuth não inicializada.".into()),
            };
            let l = g.loaded.as_ref().ok_or("Configuração não carregada.")?;
            let pos = l
                .pending_recovery
                .iter()
                .position(|entry| entry.handle == handle)
                .ok_or_else(|| "Sem credencial pendente para este identificador.".to_string())?;
            let removed = l.pending_recovery[pos].account.clone();
            let current_config = l.file.config.clone();
            (session, current_config, removed)
        };
        let oauth = match removed.oauth.as_ref() {
            Some(o) => o,
            None => {
                return Err("A conta original não tinha credencial OAuth para apagar.".to_string());
            }
        };
        let provider = removed.provider;
        let outcome =
            account_edit::purge_credential(session.as_ref(), &current_config, provider, oauth)
                .await;
        match outcome {
            Ok(()) => {
                // Success: drop the entry, surface the truthful
                // result. The handle is consumed so a stale UI
                // button cannot retry after we already deleted the
                // vault entry.
                let mut g = self.inner.lock().await;
                if let Some(l) = g.loaded.as_mut() {
                    l.pending_recovery.retain(|entry| entry.handle != handle);
                }
                Ok(DeleteResult::new("purged"))
            }
            Err(vault_error) => {
                // Refused (still shared) or vault failure. Keep the
                // handle so the UI can keep offering the retry; the
                // core guard will continue to refuse while the
                // survivor exists and succeed once it is dropped.
                Ok(DeleteResult {
                    outcome: "partial_failure",
                    other_account: None,
                    message: Some(vault_error.to_string()),
                    recovery_handle: Some(handle.to_string()),
                })
            }
        }
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
            updated
                .validate()
                .map_err(|e| l10n_to_json(&cuotai_core::edit_error::config_error_note(&e)))?;
            let new_monitor = l
                .monitor
                .with_config(updated.clone())
                .map_err(|e| l10n_to_json(&cuotai_core::edit_error::config_error_note(&e)))?;
            l.file
                .save(updated.clone())
                .map_err(|e| edit_error_to_json(&e))?;
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
                .map_err(|e| edit_error_to_json(&e))?;
            if from == to {
                return Ok(());
            }
            let mut staged_cycle = l.cycle.clone_state_for_edit();
            let due = staged_cycle
                .apply_edit(&prev, &updated, Edit::Move { from, to }, Instant::now())
                .map_err(|e| l10n_to_json(&refresh_edit_error(&e)))?;
            debug_assert!(due.is_empty(), "a move must not schedule queries");
            let new_monitor = l
                .monitor
                .with_config(updated.clone())
                .map_err(|e| l10n_to_json(&cuotai_core::edit_error::config_error_note(&e)))?;
            l.file.save(updated).map_err(|e| edit_error_to_json(&e))?;

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
        let (auth_mode, auth_options, auth_registration) = if a.oauth.is_some() {
            let request = LoginRequest::for_account(a.provider, login::MODE_OAUTH, Some(a));
            (login::MODE_OAUTH, request.options, request.registration)
        } else {
            ("credential", BTreeMap::new(), BTreeMap::new())
        };
        let native_login = a.oauth.is_some()
            && login::login_mode(a.provider, auth_mode).is_some_and(|mode| {
                mode.credential_store == login::LoginCredentialStore::CuotaiVault
                    && mode.registration.is_empty()
            });
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
                            .and_then(|value| {
                                c.options.iter().position(|option| option.value == value)
                            })
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
            native_login,
            auth_mode,
            auth_options,
            auth_registration,
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
        // RF2.8: the core helper binds the request to the edited account and
        // owns credential_id reuse plus the replaced-key cleanup context.
        let existing = input.index.and_then(|index| {
            g.loaded
                .as_ref()
                .and_then(|loaded| loaded.file.config.accounts.get(index))
        });
        let mut request = LoginRequest::for_account(input.provider, &input.mode, existing);
        request.options = input.options.clone();
        request.registration = input.registration.clone();
        // Valida o NOVO pedido ANTES de tomar o predecessor.
        let driver =
            login::start_login(session, input.provider, request).map_err(|e| e.to_string())?;
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

    // ---------- modelos (PRD 0002 RF6.1) ----------

    /// Runs a model operation of `Monitor` for account `index` of the
    /// configuration `generation` (the token the UI read with its `View`).
    /// Another generation before or after the operation (save, remove, move,
    /// reload, file switch) yields [`STALE`]: the result belongs to the old
    /// identity and is never attributed to the account now at `index`. The
    /// core validates the index; the quota cycle and its tokens are untouched.
    pub async fn models<F, Fut>(
        &self,
        index: usize,
        generation: u64,
        op: F,
    ) -> Result<ModelState, String>
    where
        F: FnOnce(Arc<Monitor>, usize) -> Fut,
        Fut: Future<Output = Result<ModelState, ModelError>>,
    {
        let monitor = {
            let g = self.inner.lock().await;
            let l = g.loaded.as_ref().ok_or("Configuração não carregada.")?;
            if l.generation != generation {
                return Err(STALE.into());
            }
            Arc::clone(&l.monitor)
        };
        let state = op(monitor, index).await;
        let live = self
            .inner
            .lock()
            .await
            .loaded
            .as_ref()
            .map(|l| l.generation);
        if live != Some(generation) {
            return Err(STALE.into());
        }
        state.map_err(|e| l10n_to_json(&e.l10n()))
    }
}

/// Erro estável dos comandos de modelos: a configuração mudou desde a view da UI.
pub const STALE: &str = "stale";

#[derive(Serialize)]
pub struct ModelSupport {
    supported: bool,
    /// Fonte do catálogo quando `supported`, senão o motivo (sem rede).
    text: cuotai_core::text::Localized,
}

/// Orientação para uma conta ainda não salva: o mesmo probe padrão do
/// assistente da TUI (sem parâmetros gravados nem rede).
pub fn model_support(provider: Provider) -> ModelSupport {
    let probe = cuotai_core::Account {
        provider,
        ..Default::default()
    };
    let (supported, text) = match models::support(&probe) {
        Ok(text) => (true, text),
        Err(text) => (false, text),
    };
    ModelSupport { supported, text }
}

// ---------- formulário ----------

#[derive(Deserialize, Serialize)]
pub struct FieldValue {
    pub key: String,
    pub text: Option<String>,
    pub choice: Option<usize>,
}

/// A interface envia os campos em camelCase. Login options/results contain
/// only public values; the core applies the selected mode's account binding.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInput {
    pub index: Option<usize>,
    pub provider: Provider,
    pub name: String,
    pub credential: String,
    pub fields: Vec<FieldValue>,
    pub interval: String,
    #[serde(default)]
    pub commit_login: bool,
    #[serde(default)]
    pub login_attempt: u64,
    #[serde(default = "default_credential_mode")]
    pub auth_mode: String,
    #[serde(default)]
    pub auth_options: BTreeMap<String, String>,
    #[serde(default)]
    pub auth_params: BTreeMap<String, String>,
}

#[derive(Serialize)]
pub struct SaveAccountResult {
    pub cleanup_warning: Option<cuotai_core::text::Localized>,
}

fn default_credential_mode() -> String {
    "credential".to_string()
}

/// Entrada de `login_start`: modo escolhido + opções/registro não secretos.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStartInput {
    pub provider: Provider,
    pub index: Option<usize>,
    pub mode: String,
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    #[serde(default)]
    pub registration: BTreeMap<String, String>,
}

#[derive(Serialize)]
pub struct AccountForm {
    name: String,
    provider: Provider,
    credential: String,
    interval: String,
    uses_oauth: bool,
    native_login: bool,
    auth_mode: &'static str,
    auth_options: BTreeMap<String, String>,
    auth_registration: BTreeMap<String, String>,
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
    index: usize,
    value: String,
    label: cuotai_core::text::Localized,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth_modes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable_reason: Option<cuotai_core::text::Localized>,
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
pub struct FieldChoiceSchema {
    key: &'static str,
    options: Vec<ChoiceSchema>,
    choice: usize,
    notice: Option<cuotai_core::text::Localized>,
}

#[derive(Serialize)]
pub struct LoginOptionSchema {
    id: String,
    key: String,
    label: cuotai_core::text::Localized,
    help: cuotai_core::text::Localized,
    choices: Option<Vec<ChoiceSchema>>,
    required: bool,
    validation: login::LoginOptionValidation,
}

#[derive(Serialize)]
pub struct LoginModeSchema {
    id: String,
    label: cuotai_core::text::Localized,
    help: cuotai_core::text::Localized,
    completion_notice: cuotai_core::text::Localized,
    credential_store: login::LoginCredentialStore,
    account_binding: login::LoginAccountBinding,
    registration: Vec<LoginOptionSchema>,
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
    credential_kind: CredentialKind,
    default_env: &'static str,
    credential_help: cuotai_core::text::Localized,
    cost_advisory: Option<cuotai_core::text::Localized>,
    fields: Vec<FieldSchema>,
    login: ProviderLoginSchema,
}

fn login_option_schema(
    provider: Provider,
    mode_id: &str,
    option: login::LoginOption,
    registration: bool,
) -> LoginOptionSchema {
    let slug = provider.slug();
    let key = option.key.clone();
    // RF8: the option descriptor already carries stable Localized ids for
    // label/help/choices (built by `LoginOption::text`/`from_native`).
    // Wrap it in a small schema envelope so the UI can still group the
    // option under the parent mode (registration vs native flow) without
    // re-deriving the ids.
    let id = if registration {
        format!("login.{slug}.{mode_id}.registration.{key}")
    } else {
        format!("native.{slug}.{key}")
    };
    let choices = option.choices.map(|choices| {
        choices
            .into_iter()
            .enumerate()
            .map(|(index, choice)| ChoiceSchema {
                id: choice.label.id.clone().into_owned(),
                index,
                value: choice.value,
                label: choice.label,
                auth_modes: choice.auth_modes,
                unavailable_reason: choice.unavailable_reason,
            })
            .collect()
    });
    LoginOptionSchema {
        id,
        key: option.key,
        label: option.label,
        help: option.help,
        choices,
        required: option.required,
        validation: option.validation,
    }
}

pub fn form_schema(provider: Provider) -> FormSchema {
    let slug = provider.slug();
    let login = login::login_modes(provider);
    FormSchema {
        credential_kind: account_edit::credential_kind(provider),
        default_env: default_env(provider),
        credential_help: credential_help(provider),
        cost_advisory: account_edit::cost_advisory(provider),
        login: ProviderLoginSchema {
            modes: login
                .modes
                .into_iter()
                .map(|mode| {
                    let mode_id = mode.id.clone();
                    LoginModeSchema {
                        id: mode.id,
                        label: mode.label,
                        help: mode.help,
                        completion_notice: mode.completion_notice,
                        credential_store: mode.credential_store,
                        account_binding: mode.account_binding,
                        registration: mode
                            .registration
                            .into_iter()
                            .map(|option| login_option_schema(provider, &mode_id, option, true))
                            .collect(),
                        options: mode
                            .options
                            .into_iter()
                            .map(|option| login_option_schema(provider, &mode_id, option, false))
                            .collect(),
                        unavailable_reason: mode.unavailable_reason,
                    }
                })
                .collect(),
            unsupported_reason: login.unsupported_reason,
        },
        fields: field_descriptors(provider)
            .iter()
            .map(|d| FieldSchema {
                id: format!("field.{slug}.{}", d.key),
                key: d.key,
                label: cuotai_core::text::Localized::fixed(
                    account_edit::field_label_id(provider, d.key),
                    d.label,
                ),
                help: cuotai_core::text::Localized::fixed(
                    account_edit::field_help_id(provider, d.key),
                    d.help,
                ),
                required: d.required,
                options: match d.kind {
                    FieldDescriptorKind::Text => None,
                    FieldDescriptorKind::Choice(c) => Some(
                        c.options
                            .iter()
                            .enumerate()
                            .map(|(index, option)| {
                                let id =
                                    account_edit::field_option_id(provider, d.key, option.value);
                                ChoiceSchema {
                                    id: id.clone(),
                                    index,
                                    value: option.value.to_owned(),
                                    label: cuotai_core::text::Localized::fixed(id, option.label),
                                    auth_modes: option.auth_modes.map(|modes| {
                                        modes.iter().map(|mode| (*mode).to_string()).collect()
                                    }),
                                    unavailable_reason: account_edit::field_option_unavailability(
                                        provider,
                                        d.key,
                                        option.value,
                                    ),
                                }
                            })
                            .collect(),
                    ),
                },
            })
            .collect(),
    }
}

/// Resolve every provider-owned choice for an authentication mode. The UI
/// round-trips full descriptor indices; filtering and substitution stay in the
/// core contract rather than being reimplemented in JavaScript.
pub fn field_choices(
    provider: Provider,
    choices: BTreeMap<String, usize>,
    auth_mode: &str,
) -> Vec<FieldChoiceSchema> {
    field_descriptors(provider)
        .iter()
        .filter_map(|descriptor| {
            let FieldDescriptorKind::Choice(choice) = descriptor.kind else {
                return None;
            };
            let current = choices
                .get(descriptor.key)
                .and_then(|index| choice.options.get(*index))
                .map(|option| option.value);
            let resolved =
                account_edit::resolve_field_choice(provider, descriptor.key, auth_mode, current)?;
            let mode_options = account_edit::field_options(provider, descriptor.key, auth_mode)?;
            let options = mode_options
                .options
                .into_iter()
                .map(|field_option| {
                    let option = &choice.options[field_option.index];
                    let id = account_edit::field_option_id(provider, descriptor.key, option.value);
                    ChoiceSchema {
                        id: id.clone(),
                        index: field_option.index,
                        value: option.value.to_string(),
                        label: cuotai_core::text::Localized::fixed(id, option.label),
                        auth_modes: option
                            .auth_modes
                            .map(|modes| modes.iter().map(|mode| (*mode).to_string()).collect()),
                        unavailable_reason: account_edit::field_option_unavailability(
                            provider,
                            descriptor.key,
                            option.value,
                        ),
                    }
                })
                .collect();
            Some(FieldChoiceSchema {
                key: descriptor.key,
                options,
                choice: resolved.option.index,
                notice: resolved.notice,
            })
        })
        .collect()
}

/// Resultado de remover uma conta, para a UI dizer a verdade sobre o cofre.
#[derive(Debug, Serialize)]
pub struct DeleteResult {
    /// "removed" (sem credencial no cofre), "purged" (apagada do cofre), "kept" (mantida a pedido),
    /// "kept_shared" (mantida porque outra conta a usa) ou "partial_failure" (TOML salvo, cofre não apagado).
    outcome: &'static str,
    other_account: Option<String>,
    message: Option<String>,
    /// RF3 handle round-tripped to `retry_purge`. Present when the
    /// credential is still in the vault (`kept`, `kept_shared`,
    /// `partial_failure`) and the UI should offer a retry button.
    /// Opaque: the UI never inspects the value, only echoes it back.
    /// Absent for `removed` and `purged` (nothing to retry).
    recovery_handle: Option<String>,
}

impl DeleteResult {
    fn new(outcome: &'static str) -> Self {
        Self {
            outcome,
            other_account: None,
            message: None,
            recovery_handle: None,
        }
    }

    fn with_recovery(mut self, handle: String) -> Self {
        self.recovery_handle = Some(handle);
        self
    }
}

/// Mint an opaque, monotonically increasing recovery handle. The
/// value is never inspected — the UI round-trips it verbatim — but a
/// fresh id per removal makes stale handles self-evident in the logs
/// and prevents the UI from replaying an old `retry_purge` after a
/// config replacement cleared the list (the lookup misses cleanly
/// instead of deleting the wrong credential).
fn mint_handle() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    // Process-global counter; the value is opaque to the UI, only the
    // uniqueness and the monotonicity matter (a stale handle after
    // config replacement just misses the lookup).
    COUNTER.fetch_add(1, Ordering::Relaxed).to_string()
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

/// Cuotai-vault modes require an authorized/existing credential. External
/// CLI modes never inherit this requirement.
fn login_needed_before_save(mode: Option<&login::LoginMode>, oauth_mode: bool) -> bool {
    mode.is_some_and(|mode| mode.credential_store == login::LoginCredentialStore::CuotaiVault)
        && !oauth_mode
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
    let mut vis = FieldVisibility::new(auth_mode, login, &choices);
    vis.native_login = login::login_mode(provider, auth_mode).is_some_and(|mode| {
        mode.credential_store == login::LoginCredentialStore::CuotaiVault
            && mode.registration.is_empty()
    });
    let d = field_descriptors(provider);
    visible_field_indices(provider, &vis)
        .into_iter()
        .map(|i| d[i].key)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuotai_core::Account;

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
        assert_eq!(
            cycle.snapshot().map(|s| s.reports.len()),
            Some(config.accounts.len())
        );

        config.refresh_seconds = 3600;
        let rebuilt = rebuild_cycle_keeping_reports(&cycle, &config);
        // relatórios preservados, nenhuma conta precisa de nova consulta
        assert_eq!(
            rebuilt.snapshot().map(|s| s.reports.len()),
            Some(config.accounts.len())
        );
        assert!(rebuilt.pending_indices().is_empty());
        // contas sem intervalo próprio passam a seguir o novo valor global (≈ 1 h)
        let next = rebuilt.next_refresh_in(0, Instant::now());
        assert!(
            next > Duration::from_secs(3000),
            "prazo não foi reagendado: {next:?}"
        );
    }

    #[test]
    fn login_start_input_reads_the_camel_case_fields_the_ui_sends() {
        let input: LoginStartInput = serde_json::from_value(serde_json::json!({
            "provider": "gemini_api", "index": null, "mode": "oauth", "options": {},
            "registration": {"client_id": "meu-client", "client_secret_env": "MEU_SEGREDO"}
        }))
        .expect("pedido de login");
        assert_eq!(
            input.registration.get("client_id").map(String::as_str),
            Some("meu-client"),
            "registration client_id foi ignorado"
        );
        assert_eq!(
            input
                .registration
                .get("client_secret_env")
                .map(String::as_str),
            Some("MEU_SEGREDO")
        );
    }

    #[test]
    fn account_input_reads_the_camel_case_fields_the_ui_sends() {
        // Exatamente o objeto que `saveAccount` do diálogo monta ao salvar depois de um login concluído.
        let input: AccountInput = serde_json::from_value(serde_json::json!({
            "index": null, "provider": "kiro", "name": "Kiro Pessoal", "credential": "",
            "fields": [], "interval": "",
            "commitLogin": true, "loginAttempt": 7, "authMode": "oauth",
            "authOptions": {"region": "us-east-1"}, "authParams": {}
        }))
        .expect("pedido do diálogo");
        assert!(input.commit_login, "commitLogin foi ignorado");
        assert_eq!(input.login_attempt, 7);
        assert_eq!(input.auth_mode, "oauth");
        assert_eq!(
            input.auth_options.get("region").map(String::as_str),
            Some("us-east-1")
        );
    }

    #[test]
    fn oauth_mode_without_login_is_reported_as_missing_login_not_missing_env_var() {
        let oauth_mode = login::login_mode(Provider::GeminiApi, login::MODE_OAUTH);
        let aws_mode = login::login_mode(Provider::AwsBedrock, login::MODE_AWS_SSO);
        let bailian_mode =
            login::login_mode(Provider::AlibabaTokenPlan, login::MODE_BAILIAN_CONSOLE);
        assert!(login_needed_before_save(oauth_mode.as_ref(), false));
        assert!(!login_needed_before_save(oauth_mode.as_ref(), true));
        assert!(!login_needed_before_save(None, false));
        assert!(!login_needed_before_save(aws_mode.as_ref(), false));
        assert!(!login_needed_before_save(bailian_mode.as_ref(), false));
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

    // -------------------------------------------------------------------
    // RF3 desktop recovery: `delete_account` registers an opaque
    // handle on `kept` / `partial_failure`, drops it on config
    // replacement, and `retry_purge` honours the core's shared-key
    // guard so a retry after the survivor drops succeeds and a retry
    // while the survivor remains is refused with the same honest
    // message the core uses everywhere else.
    // -------------------------------------------------------------------

    use cuotai_core::oauth::{OAuthConfig, OAuthToken, TokenStore};
    use parking_lot::Mutex;
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Arc;
    use tempfile::TempDir;

    /// In-memory `TokenStore` mirroring the core's RF3 test fixture.
    #[derive(Default)]
    struct MemoryStore {
        entries: Mutex<HashMap<String, OAuthToken>>,
        delete_calls: Mutex<Vec<String>>,
        fail_deletes: Mutex<u32>,
    }

    impl MemoryStore {
        #[allow(dead_code)] // exercised by future PartialFailure tests; mirrors the core fixture.
        fn fail_next_deletes(&self, n: u32) {
            *self.fail_deletes.lock() = n;
        }
    }

    impl TokenStore for MemoryStore {
        fn load(&self, id: &str) -> Result<Option<OAuthToken>, cuotai_core::oauth::OAuthError> {
            Ok(self.entries.lock().get(id).cloned())
        }
        fn save(&self, id: &str, token: &OAuthToken) -> Result<(), cuotai_core::oauth::OAuthError> {
            self.entries.lock().insert(id.into(), token.clone());
            Ok(())
        }
        fn delete(&self, id: &str) -> Result<(), cuotai_core::oauth::OAuthError> {
            self.delete_calls.lock().push(id.into());
            let mut remaining = self.fail_deletes.lock();
            if *remaining > 0 {
                *remaining -= 1;
                return Err(cuotai_core::oauth::OAuthError::Storage(
                    cuotai_core::text::Localized::fixed(
                        "oauth_error.test.keychain_unavailable",
                        "Keychain indisponível no teste",
                    ),
                ));
            }
            self.entries.lock().remove(id);
            Ok(())
        }
    }

    struct NoSecrets;
    impl cuotai_core::Credentials for NoSecrets {
        fn get(&self, _: &str) -> Option<String> {
            None
        }
    }

    fn oauth_config_for(provider: Provider) -> OAuthConfig {
        let mut config = OAuthConfig::new(String::new());
        config.credential_id = "4be708d8-7073-4d01-85ba-3306cc3cf087".into();
        config.validate(provider).unwrap();
        config
    }

    fn oauth_account(name: &str, provider: Provider) -> Account {
        Account {
            name: name.into(),
            provider,
            credential_env: None,
            oauth: Some(oauth_config_for(provider)),
            params: BTreeMap::new(),
            refresh_seconds: None,
        }
    }

    async fn save_token_for(
        store: &MemoryStore,
        provider: Provider,
        oauth: &OAuthConfig,
    ) -> String {
        let key = account_edit::account_storage_key(&Account {
            name: "probe".into(),
            provider,
            credential_env: None,
            oauth: Some(oauth.clone()),
            params: BTreeMap::new(),
            refresh_seconds: None,
        })
        .expect("OAuth config is valid; the storage key is derivable");
        store.entries.lock().insert(
            key.clone(),
            OAuthToken {
                access_token: format!("access-for-{}", oauth.credential_id),
                refresh_token: None,
                expires_at: None,
                metadata: BTreeMap::new(),
            },
        );
        key
    }

    #[tokio::test(flavor = "current_thread")]
    async fn delete_account_registers_handle_on_kept_and_drops_after_install() {
        let store = Arc::new(MemoryStore::default());
        let backend = build_backend(
            store.clone(),
            vec![oauth_account("solo", Provider::ChatGpt)],
        )
        .await;
        let oauth = backend.loaded_config().await.accounts[0]
            .oauth
            .clone()
            .unwrap();
        let key = save_token_for(&store, Provider::ChatGpt, &oauth).await;

        // `purge_credentials = true` + unique key → the staged
        // outcome is `ReadyForPurge`, the vault delete succeeds, and
        // no recovery handle is registered (nothing left to retry).
        let result = backend
            .drive_delete_account(0, true)
            .await
            .expect("delete_account must succeed");
        assert_eq!(result.outcome, "purged");
        assert!(result.recovery_handle.is_none());
        assert!(backend.pending_recovery().await.is_empty());
        assert!(store.entries.lock().get(&key).is_none());

        // `install` (config replacement) must NOT carry stale
        // recovery entries into the next session; this is the
        // invariant that protects against "retry on a credential
        // now referenced again". The test injects a fake entry,
        // reinstalls, and checks the list is empty.
        {
            let mut g = backend.inner.inner.lock().await;
            g.loaded
                .as_mut()
                .unwrap()
                .pending_recovery
                .push(RecoveryEntry {
                    handle: "stale-handle".into(),
                    account: account_edit::RemovedAccount {
                        provider: Provider::ChatGpt,
                        oauth: Some(oauth.clone()),
                        storage_key: Some(key.clone()),
                    },
                });
        }
        backend
            .reinstall_for_test(Config::default())
            .await
            .expect("reinstall");
        assert!(
            backend.pending_recovery().await.is_empty(),
            "install must drop recovery handles"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retry_purge_succeeds_after_survivor_drops_and_refuses_while_shared() {
        // Two accounts sharing the same OAuth config (same
        // credential_id → same storage key). The first removal
        // returns `kept_shared` with a recovery handle; a retry
        // must refuse while the survivor exists, then succeed once
        // the survivor is also removed.
        let store = Arc::new(MemoryStore::default());
        let backend = build_backend(
            store.clone(),
            vec![
                oauth_account("first", Provider::ChatGpt),
                oauth_account("second", Provider::ChatGpt),
            ],
        )
        .await;
        let oauth = backend.loaded_config().await.accounts[0]
            .oauth
            .clone()
            .unwrap();
        let key = save_token_for(&store, Provider::ChatGpt, &oauth).await;

        let result = backend
            .drive_delete_account(0, true)
            .await
            .expect("delete_account must succeed");
        let handle = result
            .recovery_handle
            .expect("kept_shared must register a recovery handle");
        assert_eq!(result.outcome, "kept_shared");
        assert_eq!(backend.pending_recovery().await.len(), 1);
        // Credential still in the vault (shared-key guard).
        assert!(store.entries.lock().contains_key(&key));

        // Retry while the survivor still shares the key: refused.
        let refused = backend
            .drive_retry(&handle)
            .await
            .expect("retry must return a DeleteResult, not error");
        assert_eq!(refused.outcome, "partial_failure");
        assert!(refused.recovery_handle.is_some());
        assert!(
            refused
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("sobrevive")
                || refused
                    .message
                    .as_deref()
                    .unwrap_or_default()
                    .contains("compartilh")
                || refused
                    .message
                    .as_deref()
                    .unwrap_or_default()
                    .contains("shared"),
            "refusal message must mention the survivor: {:?}",
            refused.message,
        );
        // Handle is preserved so the UI can keep offering the retry.
        assert_eq!(backend.pending_recovery().await.len(), 1);

        // Drop the survivor (same OAuth config, the config still
        // has one account after we removed it manually via a fresh
        // delete). The shared-key guard must now let the retry
        // through.
        let second_result = backend
            .drive_delete_account(0, false /* declined */)
            .await
            .expect("second removal must succeed");
        assert_eq!(second_result.outcome, "kept");
        assert!(second_result.recovery_handle.is_some());
        assert!(store.entries.lock().contains_key(&key));

        // Retry against the now-unique config must purge.
        let purged = backend
            .drive_retry(&handle)
            .await
            .expect("retry must succeed");
        assert_eq!(purged.outcome, "purged");
        assert!(purged.recovery_handle.is_none());
        assert!(store.entries.lock().get(&key).is_none());
        // Only the retried handle is consumed; the second removal's
        // handle is still in the list (it references a different
        // credential that is now alone and not shared).
        let remaining = backend.pending_recovery().await;
        assert_eq!(remaining.len(), 1);
        assert_ne!(remaining[0], handle, "retried handle must be dropped");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retry_purge_rejects_unknown_handle() {
        let store = Arc::new(MemoryStore::default());
        let backend = build_backend(
            store.clone(),
            vec![oauth_account("solo", Provider::ChatGpt)],
        )
        .await;
        let oauth = backend.loaded_config().await.accounts[0]
            .oauth
            .clone()
            .unwrap();
        save_token_for(&store, Provider::ChatGpt, &oauth).await;

        // No removal happened: no handles exist. A retry for an
        // unknown id must error (not silently succeed and not
        // accidentally delete the survivor's credential).
        let err = backend.drive_retry("never-issued").await.unwrap_err();
        assert!(
            err.contains("Sem credencial pendente") || err.contains("pendente"),
            "unexpected error message: {err}"
        );
        assert!(
            store.entries.lock().contains_key(
                &account_edit::account_storage_key(&Account {
                    name: "solo".into(),
                    provider: Provider::ChatGpt,
                    credential_env: None,
                    oauth: Some(oauth.clone()),
                    params: BTreeMap::new(),
                    refresh_seconds: None,
                })
                .unwrap()
            ),
            "an unknown-handle retry must not touch the vault"
        );
    }

    // --- test helpers (kept local; do not export) ---

    /// Build a `Backend` whose `Inner.loaded` already contains the
    /// given accounts (no `Monitor::refresh`, no OS discovery).
    async fn build_backend(store: Arc<MemoryStore>, accounts: Vec<Account>) -> TestBackend {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        std::fs::write(&path, "refresh_seconds = 300\naccounts = []\n").unwrap();
        let mut file = ConfigFile::load(Some(path), false).unwrap();
        let config = Config {
            refresh_seconds: 300,
            concurrency: 4,
            timeout_seconds: 30,
            accounts,
        };
        file.save(config.clone()).unwrap();
        let session = Arc::new(
            cuotai_core::oauth::OAuthSession::new(
                store as Arc<dyn TokenStore>,
                Arc::new(NoSecrets),
            )
            .unwrap(),
        );
        let monitor = Arc::new(
            Monitor::with_oauth(
                config.clone(),
                Arc::new(cuotai_core::EnvironmentCredentials),
                Arc::clone(&session),
            )
            .unwrap(),
        );
        let backend = Backend::default();
        let mut g = backend.inner.lock().await;
        g.generation = 1;
        g.session = Some(session);
        g.loaded = Some(Loaded {
            file,
            monitor,
            cycle: RefreshCycle::new(&config, Instant::now()),
            generation: 1,
            tokens: vec![None; config.accounts.len()],
            pending_recovery: Vec::new(),
        });
        drop(g);
        TestBackend {
            inner: backend,
            _dir: dir,
        }
    }

    /// Wrapper that lets tests reach `Backend::inner.lock().await`
    /// directly through a thin facade, avoiding the need to spin up
    /// a `tauri::AppHandle`.
    struct TestBackend {
        inner: Backend,
        _dir: TempDir,
    }

    impl TestBackend {
        async fn pending_recovery(&self) -> Vec<String> {
            let g = self.inner.inner.lock().await;
            g.loaded
                .as_ref()
                .map(|l| {
                    l.pending_recovery
                        .iter()
                        .map(|e| e.handle.clone())
                        .collect()
                })
                .unwrap_or_default()
        }
        async fn loaded_config(&self) -> Config {
            let g = self.inner.inner.lock().await;
            g.loaded.as_ref().unwrap().file.config.clone()
        }
    }

    // Trait-bridge so the test helpers above can call the
    // `pub(crate)` methods that drive the public Tauri commands
    // without a `tauri::AppHandle`.
    impl TestBackend {
        pub(crate) async fn drive_delete_account(
            &self,
            index: usize,
            purge: bool,
        ) -> Result<DeleteResult, String> {
            self.inner.drive_delete_account(index, purge).await
        }
        pub(crate) async fn drive_retry(&self, handle: &str) -> Result<DeleteResult, String> {
            self.inner.drive_retry(handle).await
        }
        pub(crate) async fn reinstall_for_test(&self, config: Config) -> Result<(), String> {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("cuotai.toml");
            std::fs::write(&path, "refresh_seconds = 300\naccounts = []\n").unwrap();
            let mut file = ConfigFile::load(Some(path), false).unwrap();
            file.save(config).unwrap();
            let mut g = self.inner.inner.lock().await;
            g.install(file)
        }
        /// File path the live `ConfigFile` was loaded from; `reload`
        /// needs a valid path or it returns the in-memory state.
        async fn file_path(&self) -> PathBuf {
            let g = self.inner.inner.lock().await;
            g.loaded.as_ref().unwrap().file.path.clone()
        }
        /// RF4 reload path: equivalent to `Backend::reload_config`
        /// but operating on the test backend. The function ignores
        /// the `AppHandle` emission (the shell isn't required to
        /// prove the reload semantics) and returns the post-reload
        /// config so the test can assert the live state.
        async fn reload_for_test(&self) -> Config {
            // Mirrors `reload_config` without a Tauri runtime: re-open the
            // live path/source, then rebuild monitor/cycle via `install`.
            let mut g = self.inner.inner.lock().await;
            let l = g.loaded.as_ref().unwrap();
            let resolved = ResolvedPath {
                path: l.file.path.clone(),
                source: l.file.source,
            };
            let file = ConfigFile::open(resolved, true)
                .expect("reload must parse the freshly-edited file");
            let new_config = file.config.clone();
            g.install(file).expect("install");
            new_config
        }
    }

    // RF1.2: desktop precedence env > chosen > OS, never `./cuotai.toml`;
    // switching reloads accounts and drops per-config state.
    #[tokio::test(flavor = "current_thread")]
    async fn desktop_switch_follows_env_chosen_os_precedence() {
        let backend = build_backend(
            Arc::new(MemoryStore::default()),
            vec![plain_account("alpha")],
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, account: &str| {
            let path = dir.path().join(name);
            std::fs::write(
                &path,
                format!("refresh_seconds = 300\n\n[[accounts]]\nname = \"{account}\"\nprovider = \"chat_gpt\"\ncredential_env = \"K\"\n"),
            )
            .unwrap();
            path
        };
        let env = write("env.toml", "from-env");
        let chosen = write("chosen.toml", "from-chosen");
        write("cuotai.toml", "from-cwd");
        let mut loc = Locations {
            env: Some(env.clone()),
            cwd: dir.path().to_path_buf(),
            config_dir: Some(dir.path().join("os")),
        };
        let mut g = backend.inner.inner.lock().await;
        let generation = g.generation;
        let live = |g: &Inner| {
            let file = &g.loaded.as_ref().unwrap().file;
            (file.path.clone(), file.source, file.config.accounts.len())
        };
        g.switch(Some(chosen.clone()), &loc).unwrap();
        assert_eq!(live(&g), (env, PathSource::Env, 1));
        assert!(g.generation > generation, "per-config state rebuilt");
        loc.env = None;
        g.switch(Some(chosen.clone()), &loc).unwrap();
        assert_eq!(live(&g), (chosen, PathSource::Chosen, 1));
        assert_eq!(
            g.loaded.as_ref().unwrap().file.config.accounts[0].name,
            "from-chosen"
        );
        g.switch(None, &loc).unwrap();
        assert_eq!(
            live(&g),
            (
                dir.path().join("os/cuotai/config.toml"),
                PathSource::System,
                0
            )
        );
    }

    // PRD 0002 RF6.1: model commands are bound to the generation of the
    // view the UI read; a result computed for an old identity is refused.
    #[tokio::test(flavor = "current_thread")]
    async fn model_commands_validate_index_and_reject_stale_generation() {
        let backend = build_backend(
            Arc::new(MemoryStore::default()),
            vec![plain_account("alpha"), plain_account("beta")],
        )
        .await;
        let b = &backend.inner;
        let state = |m: Arc<Monitor>, i: usize| async move { m.model_state(i) };
        // Current generation: the core answers (ChatGPT lists nothing, no network).
        assert!(matches!(
            b.models(1, 1, state).await,
            Ok(ModelState::Unsupported { .. })
        ));
        // Invalid index is the core's typed error, never a panic.
        let err = b.models(9, 1, state).await.unwrap_err();
        assert!(err.contains("models.error.invalid_index"), "{err}");
        // Unknown/old token is refused before running anything.
        assert_eq!(b.models(0, 0, state).await.unwrap_err(), STALE);

        // Removal while the query runs: "beta" moves to index 0, and the
        // result computed for the old generation must not be returned.
        let raced = b
            .models(1, 1, |m, i| async move {
                let result = m.model_state(i);
                b.drive_delete_account(0, false).await.unwrap();
                result
            })
            .await;
        assert_eq!(raced.unwrap_err(), STALE);
        // The old token stays stale; the new view's token works.
        assert_eq!(b.models(0, 1, state).await.unwrap_err(), STALE);
        let live = b.view().await.generation;
        assert!(live > 1);
        assert!(b.models(0, live, state).await.is_ok());
        assert!(
            b.models(1, live, state).await.is_err(),
            "only one account left"
        );
    }

    // -------------------------------------------------------------------
    // RF4 desktop reload: external edit → reload → next save succeeds.
    // Mirrors the TUI's `reload_config_after_conflict`, but driven from
    // the test backend (no `tauri::AppHandle` available).
    // -------------------------------------------------------------------

    /// Account helper that exercises only the schema fields relevant
    /// to the reload test (no OAuth, no params): a reload pick-up is a
    /// pure-config operation that does not touch the vault.
    fn plain_account(name: &str) -> Account {
        Account {
            name: name.into(),
            provider: Provider::ChatGpt,
            credential_env: Some(format!("{}_KEY", name.to_uppercase())),
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
        }
    }

    /// Write directly to the on-disk file the backend currently
    /// holds, simulating an out-of-process editor.
    async fn externally_edit(backend: &TestBackend, file_text: &str) {
        let path = backend.file_path().await;
        std::fs::write(&path, file_text).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reload_after_external_edit_makes_next_save_succeed() {
        // Start with one account; the backend loads it cleanly.
        let backend = build_backend(
            Arc::new(MemoryStore::default()),
            vec![plain_account("alpha")],
        )
        .await;
        assert_eq!(backend.loaded_config().await.accounts.len(), 1);

        // Out-of-process editor rewrites the file (added + renamed).
        externally_edit(
            &backend,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\n\n[[accounts]]\nname = \"beta\"\nprovider = \"chat_gpt\"\ncredential_env = \"BETA_KEY\"\n",
        )
        .await;

        // Backend still believes the old snapshot is on disk: any
        // save against it would now return `Conflict`.
        assert!(
            backend.inner.config_changed_on_disk().await.unwrap(),
            "changed_on_disk must fire after the external rewrite"
        );

        // Reload rebuilds the live state. No `AppHandle` here: use
        // `reload_for_test`, which mirrors the public path.
        let reloaded = backend.reload_for_test().await;
        assert_eq!(reloaded.accounts.len(), 1);
        assert_eq!(reloaded.accounts[0].name, "beta");
        assert!(
            !backend.inner.config_changed_on_disk().await.unwrap(),
            "snapshot was refreshed, so the change detector must rest"
        );

        // Saving against the fresh snapshot succeeds (no further
        // `Conflict` from the core). We exercise the same code path
        // the dialog uses: load again, mutate, save.
        let path = backend.file_path().await;
        let mut file = ConfigFile::load(Some(path.clone()), false).unwrap();
        let mut cfg = file.config.clone();
        cfg.accounts.push(plain_account("gamma"));
        file.save(cfg.clone()).unwrap();
        assert_eq!(file.config.accounts.len(), 2, "gamma now persists");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reload_clears_stale_recovery_and_pending_refresh_tokens() {
        let store = Arc::new(MemoryStore::default());
        let backend = build_backend(store.clone(), vec![plain_account("alpha")]).await;

        // Snapshot the generation we will be bumping from.
        let initial_generation = {
            let g = backend.inner.inner.lock().await;
            g.generation
        };

        // Inject a stale recovery entry and a fake busy slot to
        // prove the RF3 invariant survives the reload (the entry
        // references the previous configuration's vault key).
        {
            let mut g = backend.inner.inner.lock().await;
            let l = g.loaded.as_mut().unwrap();
            l.pending_recovery.push(RecoveryEntry {
                handle: "stale-after-reload".into(),
                account: account_edit::RemovedAccount {
                    provider: Provider::ChatGpt,
                    oauth: Some(oauth_config_for(Provider::ChatGpt)),
                    storage_key: Some("stale-key".into()),
                },
            });
            if !l.tokens.is_empty() {
                l.tokens[0] = Some(cuotai_core::refresh::Generation(99));
            }
        }

        backend.reload_for_test().await;

        let g = backend.inner.inner.lock().await;
        let l = g.loaded.as_ref().unwrap();
        assert!(
            l.pending_recovery.is_empty(),
            "RF3 recovery entries must NOT survive a config replacement"
        );
        assert!(
            l.tokens.iter().all(Option::is_none),
            "in-flight tokens of the previous generation must be cleared"
        );
        // `install` always bumps the generation counter: the
        // back-end's own dummy token (`Generation(99)`) is now
        // provably stale because the cycle's generation guard
        // discards results that don't match the live token table.
        assert_eq!(
            l.generation,
            initial_generation + 1,
            "exactly one install ran (the original load's install plus this reload)"
        );
    }
    #[test]
    fn desktop_field_choices_use_full_indices_and_report_kiro_substitution() {
        let fresh = field_choices(Provider::Kiro, BTreeMap::new(), "credential");
        let auth_method = fresh
            .iter()
            .find(|field| field.key == "auth_method")
            .expect("Kiro auth_method schema");
        assert_eq!(
            auth_method
                .options
                .iter()
                .map(|option| (option.index, option.value.as_str()))
                .collect::<Vec<_>>(),
            [
                (2, "google"),
                (3, "github"),
                (4, "import"),
                (5, "api_key"),
                (6, "external_idp")
            ]
        );
        assert_eq!(auth_method.choice, 2);
        assert!(auth_method.notice.is_none());

        let reconciled = field_choices(
            Provider::Kiro,
            BTreeMap::from([("auth_method".to_string(), 0)]),
            "credential",
        );
        let auth_method = reconciled
            .iter()
            .find(|field| field.key == "auth_method")
            .expect("Kiro auth_method schema");
        assert_eq!(auth_method.choice, 2);
        assert_eq!(
            auth_method.notice.as_ref().map(|notice| notice.id.as_ref()),
            Some("edit_notice.option_replaced_for_auth_mode")
        );
    }
}
