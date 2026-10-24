//! PRD 0018 — Updates automáticos do app desktop.
//!
//! O shell nunca fala de endereços nem de assinaturas com a interface: este módulo é o
//! único que constrói o `Updater` do plugin oficial (`tauri-plugin-updater`), deixa o
//! plugin verificar a assinatura do pacote baixado e decide o que a janela principal e a
//! bandeja mostram.
//!
//! Privacy (RF2.6): no automatic or manual check can happen before the one-time
//! answer. After "Yes", the `desktop.json` preference must still be on, no
//! environment or build key may disable it, and a package manager must not own
//! the installation (RF3.2/RF2.5). The check fetches the same public release
//! manifest for every user; the app version, operating system and architecture
//! are compared locally and are not sent with account, usage or identity data.
//!
//! A assinatura (RF4.1) é conferida pelo plugin contra a chave pública de
//! `tauri.conf.json` (`plugins.updater.pubkey`, gerada por `cargo tauri signer generate`);
//! aqui nada disso é lido, decodificado ou repassado à interface.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use cuotai_core::install_source::{detect_install_source, InstallSource};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime, Url};
use tauri_plugin_updater::UpdaterExt;

use crate::settings::{self, UpdaterPrefs};

/// Evento com o estado da atualização, para a janela principal, o popover e a bandeja.
pub const EVENT: &str = "updater";

/// RF2.1: verificação na abertura e depois uma vez a cada 24 horas.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// RF2.3/RF2.5: o link manual que o app oferece quando algo falha (rede, assinatura,
/// espaço, permissão) ou quando a instalação não pode se atualizar no lugar.
pub const MANUAL_URL: &str = "https://github.com/jmarcon/cuotai/releases/latest";

/// RF3.1: o único canal desta versão (`stable`; PRD 0018 §4 deixa o beta para depois).
pub const CHANNEL: &str = "stable";

/// RF3.2: `CUOTAI_UPDATER=off` no ambiente desliga o atualizador interno. O empacotador
/// pode gravar o mesmo desligamento na chave de build `CUOTAI_UPDATER_DEFAULT=off`.
pub const ENV_KEY: &str = "CUOTAI_UPDATER";

/// O endpoint que o plugin consulta em desenvolvimento (`latest.json` local de teste ou
/// de smoke). Em release o endpoint vem de `plugins.updater.endpoints` e é HTTPS.
#[cfg(debug_assertions)]
pub const DEV_ENDPOINT_ENV: &str = "CUOTAI_UPDATER_ENDPOINT";

/// Pasta onde o plugin instala: a que contém o `.app` no macOS e o diretório do
/// executável nos outros sistemas.
fn install_dir(exe: &Path) -> Option<PathBuf> {
    tauri_plugin_updater::extract_path_from_executable(exe).ok()
}

/// `true` quando a pasta da instalação aceita criar um arquivo. Um arquivo de teste é
/// criado e apagado: só o bit de somente leitura do diretório não basta (é o que o macOS
/// marca quando o app veio de uma imagem de disco).
fn dir_is_writable(dir: &Path) -> bool {
    let probe = dir.join(".cuotai-write-probe");
    match fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Detecta a origem desta cópia do app a partir do executável em execução.
pub fn detect() -> InstallSource {
    let exe = std::env::current_exe().unwrap_or_default();
    detect_install_source(
        &exe,
        |key| std::env::var_os(key),
        || install_dir(&exe).is_none_or(|dir| dir_is_writable(&dir)),
    )
}

/// PRD 0018 RF3.2/RF2.5: why the updater is off after the question was answered.
/// The environment takes precedence over the build key, then package ownership,
/// then the user's preference. A pending answer is a separate hard gate and is
/// never reported as an off preference.
pub fn disabled_reason(
    prefs: &UpdaterPrefs,
    env_off: bool,
    build_off: bool,
    source: InstallSource,
) -> Option<&'static str> {
    if env_off {
        return Some("env");
    }
    if build_off {
        return Some("build");
    }
    if let Some(reason) = source.updater_disabled_reason() {
        return Some(reason);
    }
    if prefs.consent_answered && !prefs.automatic {
        return Some("preference");
    }
    None
}

/// O cliente do plugin usa rustls sem provedor de criptografia; instalar o `ring` como
/// provedor padrão do processo é o que faz o HTTPS funcionar — e o que faz um
/// certificado inválido falhar. Idempotente: se já houver um provedor, o erro é ignorado.
pub fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// RF4.3: o endpoint precisa ser HTTPS. Em desenvolvimento só um servidor local
/// (`localhost`/`127.0.0.1`) em HTTP é aceito, para os testes e o smoke; em release o
/// plugin recusa qualquer esquema que não seja HTTPS, com a mesma regra.
pub fn validate_endpoint(url: &Url) -> Result<(), &'static str> {
    if url.scheme() == "https" {
        return Ok(());
    }
    #[cfg(debug_assertions)]
    if url.scheme() == "http" && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1")) {
        return Ok(());
    }
    Err("insecure_endpoint")
}

/// RF4.3: o manifesto não é assinado, então o endereço do pacote escolhido
/// também precisa ficar no canal de release deste repositório. O plugin confere
/// a assinatura dos bytes, mas sem esta guarda ainda faria o pedido a qualquer
/// host (inclusive por HTTP) antes de recusá-los.
pub fn validate_download_url(url: &Url) -> Result<(), &'static str> {
    if url.scheme() != "https" {
        return Err("insecure_endpoint");
    }
    if url.host_str() != Some("github.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("insecure_endpoint");
    }
    let Some(segments) = url.path_segments() else {
        return Err("insecure_endpoint");
    };
    let segments: Vec<&str> = segments.collect();
    if segments.len() != 6
        || segments[..4] != ["jmarcon", "cuotai", "releases", "download"]
        || segments[4].is_empty()
        || segments[5].is_empty()
        || segments[4..].iter().any(|segment| segment.contains('%'))
    {
        return Err("insecure_endpoint");
    }
    Ok(())
}

/// Os endpoints da verificação: os de `plugins.updater.endpoints` no `tauri.conf.json`,
/// ou, em desenvolvimento, o servidor local de [`DEV_ENDPOINT_ENV`].
fn endpoints_for<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<Url>, &'static str> {
    #[cfg(debug_assertions)]
    if let Ok(raw) = std::env::var(DEV_ENDPOINT_ENV) {
        let url: Url = raw.trim().parse().map_err(|_| "invalid_endpoint")?;
        validate_endpoint(&url)?;
        return Ok(vec![url]);
    }
    let endpoints = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|plugin| plugin.get("endpoints"))
        .ok_or("unconfigured")?
        .as_array()
        .ok_or("unconfigured")?;
    let mut urls = Vec::with_capacity(endpoints.len());
    for endpoint in endpoints {
        let url: Url = endpoint
            .as_str()
            .ok_or("unconfigured")?
            .parse()
            .map_err(|_| "invalid_endpoint")?;
        validate_endpoint(&url)?;
        urls.push(url);
    }
    if urls.is_empty() {
        return Err("unconfigured");
    }
    Ok(urls)
}

/// O `Updater` do plugin com os endpoints validados; `None` quando a origem e a chave
/// pública não estão configuradas (build sem `plugins.updater`) ou quando algum endpoint
/// não passa por [`validate_endpoint`].
fn updater_for<R: Runtime>(app: &AppHandle<R>) -> Option<tauri_plugin_updater::Updater> {
    ensure_crypto_provider();
    let endpoints = endpoints_for(app).ok()?;
    app.updater_builder()
        .endpoints(endpoints)
        .ok()?
        .build()
        .ok()
}

/// RF2.3: o motivo que a interface mostra quando a verificação ou a instalação falha. O
/// código é estável e traduzido; URL, corpo da resposta e chave nunca saem daqui.
pub fn error_code(error: &tauri_plugin_updater::Error) -> &'static str {
    use tauri_plugin_updater::Error as E;
    match error {
        E::Reqwest(_) | E::Network(_) | E::ReleaseNotFound => "network",
        E::InsecureTransportProtocol => "insecure_endpoint",
        E::Minisign(_)
        | E::Base64(_)
        | E::SignatureUtf8(_)
        | E::SignedVersionMismatch { .. }
        | E::MissingSignedVersion => "signature",
        E::Io(_)
        | E::TempDirNotFound
        | E::TempDirNotOnSameMountPoint
        | E::PreviousAppNotRestored(_)
        | E::DebInstallFailed
        | E::PackageInstallFailed
        | E::AuthenticationFailed
        | E::Tauri(_) => "io",
        // `Error::Extract` (falha ao desembrulhar um `.zip`) só existe quando o recurso
        // `zip` do plugin está ligado; ele está desligado (ver `Cargo.toml`), então
        // a variante nem existe e cai em `other`.
        _ => "other",
    }
}

/// Estado interno do atualizador. O manifesto cru (`raw`) permite refazer o
/// `Update` sem outra verificação na rede; ele acompanha o download para que
/// cancelar volte a `Available`, e acompanha os bytes verificados em `Ready`
/// para que a instalação só comece depois do portão de reinício (RF2.3/RF2.4).
enum Phase {
    /// Nada a fazer (ainda não verificou, ou o recurso está desligado).
    Idle,
    /// Uma verificação está em curso.
    Checking,
    /// A última verificação não achou nada novo.
    UpToDate,
    /// Há uma versão nova. Nada foi baixado: só um clique faz isso (RF2.2).
    Available {
        version: String,
        notes: Option<String>,
        date: Option<String>,
        raw: serde_json::Value,
        /// "Lembrar depois": o aviso some até a próxima verificação.
        dismissed: bool,
    },
    /// Baixando (progresso já baixado/total; `total = None` = tamanho desconhecido).
    Downloading {
        version: String,
        notes: Option<String>,
        date: Option<String>,
        raw: serde_json::Value,
        downloaded: u64,
        total: Option<u64>,
    },
    /// Instalação local em curso depois que o portão de reinício passou.
    Installing {
        version: String,
    },
    /// Pacote baixado e verificado; nenhum instalador foi iniciado ainda.
    Ready {
        version: String,
        raw: serde_json::Value,
        bytes: Vec<u8>,
    },
    Failed {
        code: &'static str,
    },
}

/// PRD 0018 RF2.2/RF2.6/RF3.1: the UI view, without an endpoint, signature or key.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct UpdateView {
    /// Whether an update request can be made now (RF2.6/RF3.2).
    pub enabled: bool,
    /// The user's Settings → Updates preference.
    pub automatic: bool,
    /// Why checks are off, when an answered choice or policy turns them off.
    pub disabled_reason: Option<&'static str>,
    pub channel: &'static str,
    pub current_version: String,
    pub last_check: Option<String>,
    /// Whether the one-time answer is still required before any request.
    pub consent_pending: bool,
    /// `idle`, `checking`, `uptodate`, `available`, `downloading`, `installing`, `ready`
    /// ou `failed`.
    pub phase: &'static str,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub date: Option<String>,
    /// Esta instalação pode ser atualizada no lugar (RF2.5).
    pub can_install: bool,
    /// Comando do gerenciador de pacotes, quando é ele quem atualiza (RF8.1).
    pub upgrade_command: Option<&'static str>,
    pub progress: Option<u8>,
    pub dismissed: bool,
    pub error: Option<&'static str>,
    pub manual_url: &'static str,
}

/// Estado do atualizador, compartilhado pelos comandos e pela tarefa de 24 h.
pub struct Service {
    phase: Mutex<Phase>,
    /// A tarefa de download/instalação em curso, para cancelar (RF2.3).
    download: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    /// Acorda a tarefa de 24 h quando a preferência muda.
    nudge: tokio::sync::Notify,
    source: InstallSource,
    /// `desktop.json` desta máquina; `None` = as preferências ficam em memória.
    prefs_file: Option<PathBuf>,
    /// Fallback for systems without a configuration directory and for tests.
    memory_prefs: Mutex<UpdaterPrefs>,
    /// `CUOTAI_UPDATER=off` no ambiente (RF3.2).
    env_off: bool,
    /// `CUOTAI_UPDATER_DEFAULT=off` no build do empacotador (RF3.2).
    build_off: bool,
    /// Marcado antes do reinício (RF2.4): as tarefas de fundo param de pedir coisas.
    stopping: Arc<AtomicBool>,
}

impl Default for Service {
    fn default() -> Self {
        Self::new()
    }
}

impl Service {
    /// Detecta a origem da instalação e prepara o serviço (RF2.1/RF2.5).
    pub fn new() -> Self {
        ensure_crypto_provider();
        Self::build(detect(), settings::settings_path(), env_off(), build_off())
    }

    fn build(
        source: InstallSource,
        prefs_file: Option<PathBuf>,
        env_off: bool,
        build_off: bool,
    ) -> Self {
        Self {
            phase: Mutex::new(Phase::Idle),
            download: Mutex::new(None),
            nudge: tokio::sync::Notify::new(),
            source,
            prefs_file,
            memory_prefs: Mutex::new(UpdaterPrefs::default()),
            env_off,
            build_off,
            stopping: Arc::new(AtomicBool::new(false)),
        }
    }

    fn prefs(&self) -> UpdaterPrefs {
        self.prefs_file
            .as_deref()
            .map(settings::read_updater)
            .unwrap_or_else(|| self.memory_prefs.lock().clone())
    }

    fn save_prefs(&self, prefs: &UpdaterPrefs) -> Result<(), &'static str> {
        match &self.prefs_file {
            Some(file) => settings::write_updater(file, prefs),
            None => {
                *self.memory_prefs.lock() = prefs.clone();
                Ok(())
            }
        }
    }

    fn requests_allowed_for(&self, prefs: &UpdaterPrefs) -> bool {
        prefs.consent_answered
            && disabled_reason(prefs, self.env_off, self.build_off, self.source).is_none()
    }

    /// `true` only after an affirmative answer and while no policy disables
    /// checks. The scheduler, manual path and internal pre-request boundary all
    /// use this RF2.6 gate.
    pub fn requests_allowed(&self) -> bool {
        self.requests_allowed_for(&self.prefs())
    }

    /// RF2.1: nunca verificou, ou já passou 24 h (uma data ilegível conta como nunca).
    pub fn check_due(&self, prefs: &UpdaterPrefs, now: chrono::DateTime<chrono::Utc>) -> bool {
        match prefs.last_check.as_deref().and_then(parse_instant) {
            None => true,
            Some(last) => {
                now.signed_duration_since(last)
                    >= chrono::TimeDelta::from_std(CHECK_INTERVAL)
                        .unwrap_or_else(|_| chrono::TimeDelta::hours(24))
            }
        }
    }

    /// O estado para a interface, sem URL, assinatura ou chave.
    pub fn view(&self) -> UpdateView {
        let prefs = self.prefs();
        let reason = disabled_reason(&prefs, self.env_off, self.build_off, self.source);
        let source = self.source;
        let phase = self.phase.lock();
        let (name, version, notes, date, progress, dismissed, error) = match &*phase {
            Phase::Idle => ("idle", None, None, None, None, false, None),
            Phase::Checking => ("checking", None, None, None, None, false, None),
            Phase::UpToDate => ("uptodate", None, None, None, None, false, None),
            Phase::Available {
                version,
                notes,
                date,
                dismissed,
                ..
            } => (
                "available",
                Some(version.clone()),
                notes.clone(),
                date.clone(),
                None,
                *dismissed,
                None,
            ),
            Phase::Downloading {
                version,
                downloaded,
                total,
                ..
            } => {
                let progress = total.filter(|total| *total > 0).map(|total| {
                    ((*downloaded as f64 / total as f64) * 100.0)
                        .clamp(0.0, 100.0)
                        .round() as u8
                });
                (
                    "downloading",
                    Some(version.clone()),
                    None,
                    None,
                    progress,
                    false,
                    None,
                )
            }
            Phase::Installing { version } => (
                "installing",
                Some(version.clone()),
                None,
                None,
                None,
                false,
                None,
            ),
            Phase::Ready { version, .. } => (
                "ready",
                Some(version.clone()),
                None,
                None,
                None,
                false,
                None,
            ),
            Phase::Failed { code } => ("failed", None, None, None, None, false, Some(*code)),
        };
        drop(phase);
        UpdateView {
            enabled: prefs.consent_answered && reason.is_none(),
            automatic: prefs.automatic,
            disabled_reason: reason,
            channel: CHANNEL,
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            last_check: prefs.last_check.clone(),
            consent_pending: reason.is_none() && !prefs.consent_answered,
            phase: name,
            version,
            notes,
            date,
            can_install: source.can_install(),
            upgrade_command: source.upgrade_command(),
            progress,
            dismissed,
            error,
            manual_url: MANUAL_URL,
        }
    }

    fn emit<R: Runtime>(&self, app: &AppHandle<R>) {
        let _ = app.emit(EVENT, self.view());
    }

    /// RF3.1: enables or disables automatic checks in `desktop.json` after the
    /// one-time answer. Off blocks every request, including *Check now* (RF3.2).
    /// Errors: `updater_consent_required`, `unwritable`.
    pub fn set_automatic<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        on: bool,
    ) -> Result<UpdateView, String> {
        let mut prefs = self.prefs();
        if !prefs.consent_answered {
            return Err("updater_consent_required".to_string());
        }
        prefs.automatic = on;
        self.save_prefs(&prefs).map_err(str::to_owned)?;
        self.emit(app);
        self.nudge.notify_one();
        Ok(self.view())
    }

    /// RF2.6: persists the one-time answer. "Yes" enables the schedule; "No"
    /// leaves every check off. The scheduler wakes only after persistence succeeds.
    pub fn answer_automatic<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        on: bool,
    ) -> Result<UpdateView, String> {
        let mut prefs = self.prefs();
        prefs.consent_answered = true;
        prefs.automatic = on;
        self.save_prefs(&prefs).map_err(str::to_owned)?;
        self.emit(app);
        self.nudge.notify_one();
        Ok(self.view())
    }

    /// RF2.2: "Lembrar depois" — o aviso some até a próxima verificação.
    pub fn dismiss<R: Runtime>(&self, app: &AppHandle<R>) -> UpdateView {
        if let Phase::Available { dismissed, .. } = &mut *self.phase.lock() {
            *dismissed = true;
        }
        self.emit(app);
        self.view()
    }

    /// RF3.1/RF2.6: *Check now*. Before the answer or while off, it refuses
    /// without making a request.
    pub async fn check_now<R: Runtime>(&self, app: &AppHandle<R>) -> Result<UpdateView, String> {
        let prefs = self.prefs();
        if disabled_reason(&prefs, self.env_off, self.build_off, self.source).is_some() {
            return Err("updater_off".to_string());
        }
        if !prefs.consent_answered {
            return Err("updater_consent_required".to_string());
        }
        self.check(app).await;
        Ok(self.view())
    }

    /// RF2.1/RF2.6: a 24-hour scheduler tick checks only after an affirmative
    /// answer, when due and while no policy disables it.
    pub(crate) async fn tick<R: Runtime>(&self, app: &AppHandle<R>) {
        let prefs = self.prefs();
        if !self.requests_allowed_for(&prefs) || !self.check_due(&prefs, chrono::Utc::now()) {
            return;
        }
        self.check(app).await;
    }

    /// Repeats the RF2.6 gate immediately before constructing a request, marks
    /// the phase as `checking`, asks the plugin for the manifest and stores the
    /// result. Nothing downloads here (RF2.2).
    async fn check<R: Runtime>(&self, app: &AppHandle<R>) {
        if !self.requests_allowed() {
            return;
        }
        {
            let mut phase = self.phase.lock();
            if matches!(
                &*phase,
                Phase::Checking | Phase::Downloading { .. } | Phase::Installing { .. }
            ) {
                return;
            }
            *phase = Phase::Checking;
        }
        self.emit(app);
        let outcome = self.check_inner(app).await;
        *self.phase.lock() = outcome;
        // A data da última verificação (RF3.1) é gravada mesmo quando falhou: o usuário
        // precisa saber que o app tentou. Falha ao gravar não derruba a verificação.
        let mut prefs = self.prefs();
        prefs.last_check = Some(chrono::Utc::now().to_rfc3339());
        let _ = self.save_prefs(&prefs);
        self.emit(app);
    }

    async fn check_inner<R: Runtime>(&self, app: &AppHandle<R>) -> Phase {
        let Some(updater) = updater_for(app) else {
            return Phase::Failed {
                code: "unconfigured",
            };
        };
        match updater.check().await {
            Ok(Some(update)) => {
                if let Err(code) = validate_download_url(&update.download_url) {
                    return Phase::Failed { code };
                }
                Phase::Available {
                    version: update.version.clone(),
                    notes: update.body.clone(),
                    // RF1.4: `pub_date` é o que o manifesto traz; a interface formata.
                    date: update
                        .raw_json
                        .get("pub_date")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                    raw: update.raw_json.clone(),
                    dismissed: false,
                }
            }
            Ok(None) => Phase::UpToDate,
            Err(error) => Phase::Failed {
                code: error_code(&error),
            },
        }
    }

    /// RF2.3: "Atualizar agora". Sem este comando nada baixa: baixa com
    /// progresso cancelável e o plugin verifica a assinatura. Os bytes ficam
    /// em `Ready`; instalar e reiniciar só acontece em [`Self::restart`],
    /// depois do portão de login e da persistência do estado.
    pub fn install<R: Runtime>(self: &Arc<Self>, app: &AppHandle<R>) -> Result<UpdateView, String> {
        let (raw, version, notes, date) = {
            let phase = self.phase.lock();
            match &*phase {
                Phase::Available {
                    raw,
                    version,
                    notes,
                    date,
                    ..
                } => (raw.clone(), version.clone(), notes.clone(), date.clone()),
                Phase::Downloading { .. } | Phase::Installing { .. } => {
                    return Err("already_running".to_string())
                }
                _ => return Err("no_update".to_string()),
            }
        };
        let Some(updater) = updater_for(app) else {
            return Err("unavailable".to_string());
        };
        let Ok(Some(update)) = updater.restore_update(raw.clone()) else {
            return Err("stale".to_string());
        };
        validate_download_url(&update.download_url).map_err(str::to_owned)?;
        *self.phase.lock() = Phase::Downloading {
            version: version.clone(),
            notes,
            date,
            raw: raw.clone(),
            downloaded: 0,
            total: None,
        };
        self.emit(app);

        let service = ServiceHandle(Arc::clone(self));
        let app = app.clone();
        let handle = tauri::async_runtime::spawn(async move {
            // `download` confere a assinatura (RF2.3). Não chama `install`
            // aqui: no Windows isso inicia o instalador e encerra o processo.
            match update
                .download(|chunk, total| service.chunk(&app, chunk, total), || {})
                .await
            {
                Err(error) => service.fail(&app, error_code(&error)),
                Ok(bytes) => service.set_phase(
                    &app,
                    Phase::Ready {
                        version,
                        raw,
                        bytes,
                    },
                ),
            }
            *service.0.download.lock() = None;
        });
        *self.download.lock() = Some(handle);
        Ok(self.view())
    }

    /// RF2.3: cancela o download em curso; a versão instalada continua
    /// intacta e a atualização anunciada volta sem outra consulta à rede.
    pub fn cancel_download<R: Runtime>(&self, app: &AppHandle<R>) -> UpdateView {
        if let Some(handle) = self.download.lock().take() {
            handle.abort();
        }
        let mut phase = self.phase.lock();
        let previous = std::mem::replace(&mut *phase, Phase::Idle);
        *phase = match previous {
            Phase::Downloading {
                version,
                notes,
                date,
                raw,
                ..
            } => Phase::Available {
                version,
                notes,
                date,
                raw,
                dismissed: false,
            },
            other => other,
        };
        drop(phase);
        self.emit(app);
        self.view()
    }

    /// RF2.4: instala e reinicia para terminar a atualização. `prepare` é o
    /// gancho do backend (interromper o ciclo com cuidado e persistir o estado)
    /// e `login_in_progress` bloqueia antes de qualquer instalador ser iniciado.
    pub fn restart<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        login_in_progress: bool,
        prepare: impl FnOnce(),
    ) -> Result<(), String> {
        if login_in_progress {
            return Err("login_in_progress".to_string());
        }
        let (version, raw, bytes) = {
            let mut phase = self.phase.lock();
            let previous = std::mem::replace(&mut *phase, Phase::Idle);
            match previous {
                Phase::Ready {
                    version,
                    raw,
                    bytes,
                } => (version, raw, bytes),
                other => {
                    *phase = other;
                    return Err("no_update".to_string());
                }
            }
        };
        let Some(updater) = updater_for(app) else {
            *self.phase.lock() = Phase::Ready {
                version,
                raw,
                bytes,
            };
            return Err("unavailable".to_string());
        };
        let Ok(Some(update)) = updater.restore_update(raw.clone()) else {
            *self.phase.lock() = Phase::Ready {
                version,
                raw,
                bytes,
            };
            return Err("stale".to_string());
        };
        if let Err(code) = validate_download_url(&update.download_url) {
            *self.phase.lock() = Phase::Ready {
                version,
                raw,
                bytes,
            };
            return Err(code.to_string());
        }
        *self.phase.lock() = Phase::Installing {
            version: version.clone(),
        };
        self.emit(app);
        prepare();
        self.stopping.store(true, Ordering::Relaxed);
        // On Windows `install` launches NSIS/MSI and exits immediately, so
        // cleanup must happen first. On the other platforms it returns and the
        // explicit restart below finishes the same flow.
        app.cleanup_before_exit();
        match update.install(&bytes) {
            Ok(()) => app.restart(),
            Err(error) => {
                self.stopping.store(false, Ordering::Relaxed);
                let code = error_code(&error);
                *self.phase.lock() = Phase::Failed { code };
                self.emit(app);
                Err(code.to_string())
            }
        }
    }

    /// PRD 0018 RF2.1/RF2.6: after the affirmative answer, checks on opening and
    /// every 24 hours while enabled. The wait ends at the next due check or on a
    /// notification (*Check now* checks directly; answering the question also
    /// wakes the task).
    pub async fn run(self: Arc<Self>, app: AppHandle) {
        loop {
            if self.stopping.load(Ordering::Relaxed) {
                return;
            }
            self.tick(&app).await;
            let wait = self.until_next_check();
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                () = self.nudge.notified() => {}
            }
        }
    }

    /// Time until the next automatic check (never less than an instant).
    fn until_next_check(&self) -> Duration {
        let now = chrono::Utc::now();
        let prefs = self.prefs();
        // While off or unanswered, no request time is accrued. Re-evaluate hourly;
        // an answer or preference change wakes the task immediately.
        if !self.requests_allowed_for(&prefs) {
            return Duration::from_secs(60 * 60);
        }
        let Some(last) = prefs.last_check.as_deref().and_then(parse_instant) else {
            return Duration::ZERO;
        };
        let interval = chrono::TimeDelta::from_std(CHECK_INTERVAL)
            .unwrap_or_else(|_| chrono::TimeDelta::hours(24));
        let elapsed = now.signed_duration_since(last);
        if elapsed <= chrono::TimeDelta::zero() {
            // A data gravada está no futuro (relógio ajustado para trás): espera um dia.
            return CHECK_INTERVAL;
        }
        (interval - elapsed)
            .to_std()
            .unwrap_or(CHECK_INTERVAL)
            .max(Duration::from_secs(1))
    }
}

/// Ponte para a tarefa de download: o estado é sempre tomado em blocos curtos (nenhum
/// `await` com o cadeado do estado) e cada mudança avisa a interface.
struct ServiceHandle(Arc<Service>);

impl ServiceHandle {
    fn chunk<R: Runtime>(&self, app: &AppHandle<R>, chunk: usize, total: Option<u64>) {
        let mut phase = self.0.phase.lock();
        if let Phase::Downloading {
            downloaded,
            total: known,
            ..
        } = &mut *phase
        {
            *downloaded += chunk as u64;
            if known.is_none() {
                *known = total;
            }
        }
        drop(phase);
        self.0.emit(app);
    }

    fn set_phase<R: Runtime>(&self, app: &AppHandle<R>, next: Phase) {
        *self.0.phase.lock() = next;
        self.0.emit(app);
    }

    fn fail<R: Runtime>(&self, app: &AppHandle<R>, code: &'static str) {
        self.set_phase(app, Phase::Failed { code });
    }
}

/// `CUOTAI_UPDATER=off` (o valor é lido uma vez, na abertura).
fn env_off() -> bool {
    std::env::var(ENV_KEY).is_ok_and(|value| value.trim() == "off")
}

/// `CUOTAI_UPDATER_DEFAULT=off` definido pelo empacotador no momento do build.
fn build_off() -> bool {
    option_env!("CUOTAI_UPDATER_DEFAULT").is_some_and(|value| value.trim() == "off")
}

/// RFC 3339 → instante; um texto malformado vale como ausente (leitura leniente).
fn parse_instant(text: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(text.trim())
        .ok()
        .map(|at| at.with_timezone(&chrono::Utc))
}

#[cfg(test)]
mod detector {
    use super::{disabled_reason, Service, UpdaterPrefs};
    use cuotai_core::install_source::{detect_install_source, InstallSource};
    use std::{ffi::OsString, path::Path};

    fn source(path: &str, environment: &[(&str, &str)], writable: bool) -> InstallSource {
        detect_install_source(
            Path::new(path),
            |key| {
                environment
                    .iter()
                    .find_map(|(name, value)| (*name == key).then(|| OsString::from(value)))
            },
            || writable,
        )
    }

    /// The desktop consumes the core result without translating it into a
    /// second source model: updater policy and package-manager guidance stay
    /// byte-for-byte the same as before the detector moved.
    #[test]
    fn shared_sources_keep_the_desktop_updater_policy() {
        let cases = [
            (
                source(
                    "/Applications/Cuotai.app/Contents/MacOS/Cuotai",
                    &[],
                    true,
                ),
                None,
                None,
                true,
            ),
            (
                source(
                    "/usr/local/bin/Cuotai",
                    &[("APPIMAGE", "/tmp/Cuotai.AppImage")],
                    true,
                ),
                None,
                None,
                true,
            ),
            (
                source(
                    "/Volumes/Cuotai/Cuotai.app/Contents/MacOS/Cuotai",
                    &[],
                    false,
                ),
                None,
                None,
                false,
            ),
            (
                source(
                    "/private/var/folders/ab/AppTranslocation/1234/d/Cuotai.app/Contents/MacOS/Cuotai",
                    &[],
                    true,
                ),
                None,
                None,
                false,
            ),
            (
                source("/opt/Cuotai/Cuotai", &[], false),
                None,
                None,
                false,
            ),
            (
                source(
                    "/opt/homebrew/Caskroom/cuotai/0.76.0/Cuotai.app/Contents/MacOS/Cuotai",
                    &[],
                    true,
                ),
                Some("homebrew"),
                Some("brew upgrade --cask cuotai"),
                false,
            ),
            (
                source(
                    "C:/Users/demo/Scoop/Apps/Cuotai/current/Cuotai.exe",
                    &[],
                    true,
                ),
                Some("scoop"),
                Some("scoop update cuotai"),
                false,
            ),
            (
                source(
                    "C:/Users/demo/AppData/Local/Microsoft/WinGet/Packages/Cuotai.Cuotai_x/Cuotai.exe",
                    &[],
                    true,
                ),
                Some("winget"),
                Some("winget upgrade Cuotai.Cuotai"),
                false,
            ),
            (
                source("/usr/sbin/cuotai", &[], true),
                Some("linux_package"),
                None,
                false,
            ),
        ];

        for (source, reason, command, can_install) in cases {
            let view = Service::build(source, None, false, false).view();
            assert_eq!(view.disabled_reason, reason, "{source:?}");
            assert_eq!(view.upgrade_command, command, "{source:?}");
            assert_eq!(view.can_install, can_install, "{source:?}");
        }
    }

    fn prefs(automatic: bool) -> UpdaterPrefs {
        UpdaterPrefs {
            automatic,
            consent_answered: true,
            ..UpdaterPrefs::default()
        }
    }

    #[test]
    fn every_reason_stops_every_request_in_the_same_precedence() {
        assert_eq!(
            disabled_reason(&prefs(false), true, true, InstallSource::Homebrew),
            Some("env")
        );
        assert_eq!(
            disabled_reason(&prefs(false), false, true, InstallSource::Scoop),
            Some("build")
        );
        assert_eq!(
            disabled_reason(&prefs(false), false, false, InstallSource::Winget),
            Some("winget")
        );
        assert_eq!(
            disabled_reason(&prefs(false), false, false, InstallSource::LinuxPackage),
            Some("linux_package")
        );
        assert_eq!(
            disabled_reason(&prefs(false), false, false, InstallSource::Direct),
            Some("preference")
        );
        assert_eq!(
            disabled_reason(&prefs(true), false, false, InstallSource::Direct),
            None
        );
        assert_eq!(
            disabled_reason(&prefs(true), false, false, InstallSource::DiskImage),
            None
        );
    }
}

#[cfg(test)]
mod schedule {
    use super::{parse_instant, Service, UpdaterPrefs};

    fn service() -> Service {
        Service::build(super::InstallSource::Direct, None, false, false)
    }

    #[test]
    fn a_check_is_due_without_history_and_every_24_hours() {
        let now = chrono::Utc::now();
        assert!(service().check_due(&UpdaterPrefs::default(), now));
        let fresh = UpdaterPrefs {
            last_check: Some((now - chrono::TimeDelta::hours(1)).to_rfc3339()),
            ..UpdaterPrefs::default()
        };
        assert!(!service().check_due(&fresh, now));
        let old = UpdaterPrefs {
            last_check: Some((now - chrono::TimeDelta::hours(25)).to_rfc3339()),
            ..UpdaterPrefs::default()
        };
        assert!(service().check_due(&old, now));
        // Um texto ilegível vale como "nunca verificou".
        assert!(service().check_due(
            &UpdaterPrefs {
                last_check: Some("quando?".into()),
                ..UpdaterPrefs::default()
            },
            now
        ));
        // Uma data no futuro (relógio corrigido para trás) não vira laço apertado.
        assert!(!service().check_due(
            &UpdaterPrefs {
                last_check: Some((now + chrono::TimeDelta::hours(2)).to_rfc3339()),
                ..UpdaterPrefs::default()
            },
            now
        ));
    }

    #[test]
    fn instants_are_read_leniently() {
        assert!(parse_instant("2026-10-21T10:00:00Z").is_some());
        assert!(parse_instant(" 2026-10-21T10:00:00+02:00 ").is_some());
        assert!(parse_instant("ontem").is_none());
        assert!(parse_instant("").is_none());
    }
}

#[cfg(test)]
mod endpoint {
    use super::{validate_download_url, validate_endpoint, Url};

    #[test]
    fn only_https_endpoints_are_refused_when_they_are_not_local() {
        assert_eq!(
            validate_endpoint(&"http://releases.example.com/latest.json".parse().unwrap()),
            Err("insecure_endpoint")
        );
        assert_eq!(
            validate_endpoint(
                &"https://github.com/jmarcon/cuotai/releases/latest/download/latest.json"
                    .parse()
                    .unwrap()
            ),
            Ok(())
        );
        assert!(
            validate_endpoint(&"ftp://example.com/latest.json".parse::<Url>().unwrap()).is_err()
        );
    }

    #[test]
    fn downloads_are_only_from_this_repositories_https_releases() {
        let valid: Url =
            "https://github.com/jmarcon/cuotai/releases/download/v0.76.0/Cuotai_0.76.0_x64-setup.exe"
                .parse()
                .unwrap();
        assert_eq!(validate_download_url(&valid), Ok(()));

        for unsafe_url in [
            "http://github.com/jmarcon/cuotai/releases/download/v0.76.0/Cuotai.exe",
            "https://example.com/jmarcon/cuotai/releases/download/v0.76.0/Cuotai.exe",
            "https://github.com/other/cuotai/releases/download/v0.76.0/Cuotai.exe",
            "https://github.com/jmarcon/cuotai/releases/latest/download/Cuotai.exe",
            "https://github.com/jmarcon/cuotai/releases/download/v0.76.0/Cuotai.exe?mirror=1",
        ] {
            let url: Url = unsafe_url.parse().unwrap();
            assert_eq!(
                validate_download_url(&url),
                Err("insecure_endpoint"),
                "{unsafe_url}"
            );
        }
    }
    #[test]
    fn a_development_server_on_localhost_is_the_only_http_exception() {
        for host in [
            "http://127.0.0.1:8080/latest.json",
            "http://localhost:9/latest.json",
        ] {
            assert_eq!(
                validate_endpoint(&host.parse::<Url>().unwrap()),
                Ok(()),
                "{host}"
            );
        }
    }
}

#[cfg(test)]
mod failure_codes {
    use super::error_code;
    use tauri_plugin_updater::Error as E;

    #[test]
    fn each_failure_is_one_of_few_codes() {
        // Os motivos que a interface distingue (RF2.3) e nenhum detalhe de rede.
        assert_eq!(error_code(&E::ReleaseNotFound), "network");
        assert_eq!(
            error_code(&E::InsecureTransportProtocol),
            "insecure_endpoint"
        );
        assert_eq!(
            error_code(&E::SignatureUtf8("não é uma assinatura".into())),
            "signature"
        );
        assert_eq!(error_code(&E::MissingSignedVersion), "signature");
        assert_eq!(
            error_code(&E::Io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "/Applications: denied"
            ))),
            "io"
        );
        assert_eq!(error_code(&E::DebInstallFailed), "io");
        assert_eq!(error_code(&E::UnsupportedOs), "other");
        assert_eq!(error_code(&E::UnsupportedArch), "other");
    }
}
/// PRD 0018 RF1.4/RF4.3: o plugin contra um servidor local, com um par de chaves
/// efêmero gerado no teste. A chave de release nunca é lida aqui: nenhum arquivo do
/// `~/.tauri` é aberto e nenhum segredo entra no repositório.
#[cfg(test)]
mod contract {
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::{TcpListener, TcpStream},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };

    use base64::Engine as _;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    use super::*;

    /// O par de chaves efêmero do teste, na mesma forma de um `cargo tauri signer
    /// generate` (é o `signer sign` do pipeline que assina os pacotes de um release).
    struct Ephemeral {
        /// `plugins.updater.pubkey`: a chave pública em base64, como o plugin a lê.
        pubkey: String,
        pair: minisign::KeyPair,
    }

    fn keypair() -> Ephemeral {
        let pair = minisign::KeyPair::generate_unencrypted_keypair().expect("chave efêmera");
        // A chave pública no formato de arquivo (`untrusted comment: …` + a linha em
        // base64), que é o que `plugins.updater.pubkey` guarda em base64.
        let public = format!(
            "untrusted comment: minisign public key: {:016X}\n{}\n",
            u64::from_be_bytes(pair.pk.keynum().try_into().expect("identificador da chave"),),
            ct_base64(pair.pk.to_bytes().as_slice()),
        );
        Ephemeral {
            pubkey: b64(public.as_bytes()),
            pair,
        }
    }

    /// Base64 padrão, como o minisign escreve as suas linhas.
    fn ct_base64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// Assina `payload` para a versão `version`, como o CLI assina o pacote de um
    /// release (o `version:` do comentário confiável é o que liga a assinatura ao
    /// número que o manifesto anuncia).
    fn sign(key: &Ephemeral, payload: &[u8], version: &str) -> String {
        let trusted = format!(
            "timestamp:{}\tfile:Cuotai.app.tar.gz\tversion:{version}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or_default()
        );
        let boxed = minisign::sign(
            Some(&key.pair.pk),
            &key.pair.sk,
            payload,
            Some(&trusted),
            None,
        )
        .expect("assinatura");
        b64(boxed.to_string().as_bytes())
    }

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// Um servidor local que conta os pedidos e responde sempre o mesmo corpo.
    struct Local {
        url: String,
        requests: Arc<AtomicUsize>,
    }

    impl Local {
        fn http(body: String) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            let address = listener.local_addr().expect("addr");
            let requests = Arc::new(AtomicUsize::new(0));
            let counter = Arc::clone(&requests);
            std::thread::spawn(move || {
                for connection in listener.incoming() {
                    counter.fetch_add(1, Ordering::SeqCst);
                    let Ok(mut socket) = connection else { continue };
                    if reply(&mut socket, &body).is_err() {
                        continue;
                    }
                }
            });
            Self {
                url: format!("http://{address}/latest.json"),
                requests,
            }
        }

        fn count(&self) -> usize {
            self.requests.load(Ordering::SeqCst)
        }
    }

    /// Uma resposta HTTP mínima (o cliente fecha a conexão depois).
    fn reply(socket: &mut TcpStream, body: &str) -> std::io::Result<()> {
        socket.set_read_timeout(Some(Duration::from_secs(5)))?;
        let mut reader = BufReader::new(socket.try_clone()?);
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line)? == 0 || line == "\r\n" {
                break;
            }
        }
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )?;
        socket.flush()
    }

    /// O app de teste: o plugin registrado e a chave e o endpoint do servidor local na
    /// configuração (o mesmo caminho de produção — `plugins.updater` no `tauri.conf.json`).
    fn app(pubkey: &str, endpoint: &str) -> tauri::App<tauri::test::MockRuntime> {
        let mut context = mock_context(noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".to_string(),
            serde_json::json!({ "pubkey": pubkey, "endpoints": [endpoint] }),
        );
        mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .expect("app de teste")
    }

    /// Um `latest.json` de uma versão `version`, assinado com `signature`, para o
    /// alvo do plugin nesta máquina (`darwin-aarch64`, `linux-x86_64`, …).
    fn manifest(version: &str, signature: &str, download: &str) -> String {
        let target = tauri_plugin_updater::target().expect("alvo suportado");
        serde_json::json!({
            "version": version,
            "notes": "Correções e melhorias desta versão.",
            "pub_date": "2026-10-22T12:00:00Z",
            "platforms": { target: { "signature": signature, "url": download } }
        })
        .to_string()
    }

    /// O app mock responde `0.1.0` como versão atual (a do contexto de teste do Tauri).
    const CURRENT: &str = "0.1.0";

    /// A verificação real do serviço: os endpoints saem da configuração do app, como em
    /// produção.
    fn check(
        app: &tauri::App<tauri::test::MockRuntime>,
    ) -> tauri_plugin_updater::Result<Option<tauri_plugin_updater::Update>> {
        let updater = updater_for(app.handle()).expect("updater");
        tauri::async_runtime::block_on(updater.check())
    }

    #[test]
    fn a_greater_version_with_a_valid_signature_is_an_update_that_verifies() {
        let key = keypair();
        let payload = b"payload do app atualizado";
        let newer = "0.2.0";
        let manifest = manifest(
            newer,
            &sign(&key, payload, newer),
            "http://127.0.0.1:1/app.tar.gz",
        );
        let server = Local::http(manifest);
        let app = app(&key.pubkey, &server.url);
        ensure_crypto_provider();

        let update = check(&app)
            .expect("manifesto lido")
            .expect("há atualização");
        assert_eq!(update.version, newer);
        assert_eq!(update.current_version, CURRENT);
        assert_eq!(
            update.body.as_deref(),
            Some("Correções e melhorias desta versão.")
        );
        // A assinatura vale para os bytes do pacote (RF1.4) e é recusada se mudarem.
        update.verify(payload).expect("assinatura válida");
        assert!(update.verify(b"outro pacote").is_err());
        assert_eq!(server.count(), 1);
    }

    #[test]
    fn an_invalid_signature_is_refused_when_the_package_arrives() {
        let key = keypair();
        let other = keypair();
        let payload = b"payload do app atualizado";
        let newer = "0.2.0";
        let signed_by_another_key = manifest(
            newer,
            &sign(&other, payload, newer),
            "http://127.0.0.1:1/app.tar.gz",
        );
        let server = Local::http(signed_by_another_key);
        let forged = app(&key.pubkey, &server.url);

        // O manifesto é lido (ele mesmo não é assinado): o que não passa é o pacote.
        let update = check(&forged)
            .expect("manifesto lido")
            .expect("há atualização");
        let error = update
            .verify(payload)
            .expect_err("assinatura de outra chave");
        assert!(matches!(error, tauri_plugin_updater::Error::Minisign(_)));
        // Um texto que nem é assinatura também é recusado, sem instalar nada.
        let not_a_signature = manifest(
            newer,
            &b64(b"nao e uma assinatura"),
            "http://127.0.0.1:1/app.tar.gz",
        );
        let server = Local::http(not_a_signature);
        let broken = app(&key.pubkey, &server.url);
        let update = check(&broken)
            .expect("manifesto lido")
            .expect("há atualização");
        assert!(update.verify(payload).is_err());
        assert!(update.install(b"qualquer pacote").is_err());
    }

    #[test]
    fn the_same_or_a_lower_version_is_ignored() {
        let key = keypair();
        for version in [CURRENT, "0.0.9"] {
            let manifest = manifest(
                version,
                &sign(&key, b"payload", version),
                "http://127.0.0.1:1/app.tar.gz",
            );
            let server = Local::http(manifest);
            let app = app(&key.pubkey, &server.url);
            assert!(
                check(&app).expect("manifesto lido").is_none(),
                "{version} não é atualização"
            );
            assert_eq!(server.count(), 1);
        }
    }

    #[test]
    fn a_malformed_manifest_is_a_handled_error() {
        let key = keypair();
        let cases = [
            // Não é JSON.
            ("nao é json", "network"),
            // É JSON, mas não é um manifesto.
            (r#"{"foo":"bar"}"#, "other"),
            // Versão que não é semver.
            (
                r#"{"version":"depois","notes":"x","platforms":{}}"#,
                "other",
            ),
        ];
        for (body, _) in cases {
            let server = Local::http(body.to_string());
            let app = app(&key.pubkey, &server.url);
            assert!(check(&app).is_err(), "{body}");
        }
        // Um manifesto sem a entrada desta plataforma também é um erro tratado.
        let server = Local::http(r#"{"version":"0.2.0","platforms":{}}"#.to_string());
        let app = app(&key.pubkey, &server.url);
        match check(&app) {
            Ok(_found) => panic!("plataforma ausente não pode virar atualização"),
            Err(error) => assert!(
                matches!(
                    error,
                    tauri_plugin_updater::Error::TargetNotFound(_)
                        | tauri_plugin_updater::Error::TargetsNotFound(_)
                ),
                "{error}"
            ),
        }
    }

    #[test]
    fn an_http_endpoint_outside_localhost_is_refused_before_any_request() {
        let key = keypair();
        // A configuração de produção é HTTPS; o plugin recusa HTTP em release e esta
        // guarda recusa em qualquer build, então nada sai daqui.
        let app = app(&key.pubkey, "http://releases.example.invalid/latest.json");
        assert!(updater_for(app.handle()).is_none());
        let url: tauri::Url = "http://releases.example.invalid/latest.json"
            .parse()
            .unwrap();
        assert_eq!(validate_endpoint(&url), Err("insecure_endpoint"));
    }

    #[test]
    fn an_invalid_certificate_is_an_error() {
        ensure_crypto_provider();
        let key = keypair();
        let payload = b"payload";
        let newer = "0.2.0";
        let tls = TlsServer::start();
        let manifest = manifest(
            newer,
            &sign(&key, payload, newer),
            &format!("https://127.0.0.1:{}/app.tar.gz", tls.port()),
        );
        let url = tls.url();
        tls.serve(manifest);
        let app = app(&key.pubkey, &url);
        // O certificado é autoassinado e não está em nenhuma lista: o erro é do TLS, e
        // nada é instalado.
        match check(&app) {
            Ok(_found) => panic!("certificado inválido não pode virar atualização"),
            Err(tauri_plugin_updater::Error::Reqwest(error)) => {
                let mut detail = error.to_string();
                let mut source = std::error::Error::source(&error);
                while let Some(cause) = source {
                    detail.push_str(": ");
                    detail.push_str(&cause.to_string());
                    source = cause.source();
                }
                let detail = detail.to_ascii_lowercase();
                assert!(
                    detail.contains("certificate")
                        || detail.contains("unknownissuer")
                        || detail.contains("unknown issuer"),
                    "{detail}"
                );
            }
            Err(error) => panic!("expected an untrusted-certificate error, got {error}"),
        }
    }

    /// PRD 0018 RF2.6: no legacy/default/malformed unanswered state may
    /// reach the manifest server, through either the scheduler or Check now.
    #[test]
    fn every_unanswered_state_blocks_scheduled_and_manual_requests() {
        let key = keypair();
        let manifest = manifest(
            "0.2.0",
            &sign(&key, b"payload", "0.2.0"),
            "https://github.com/jmarcon/cuotai/releases/download/v0.2.0/Cuotai.app.tar.gz",
        );
        let cases = [
            ("absent", None),
            (
                "legacy_on_unseen",
                Some(r#"{"updater":{"automatic":true,"explanation_seen":false}}"#),
            ),
            (
                "legacy_on_seen",
                Some(
                    r#"{"updater":{"automatic":true,"last_check":"2025-01-01T00:00:00Z","explanation_seen":true}}"#,
                ),
            ),
            (
                "explicit_unanswered",
                Some(
                    r#"{"updater":{"automatic":true,"last_check":"2099-01-01T00:00:00Z","consent_answered":false}}"#,
                ),
            ),
            (
                "malformed",
                Some(r#"{"updater":{"automatic":"yes","consent_answered":"yes"}}"#),
            ),
        ];

        for (name, stored) in cases {
            let server = Local::http(manifest.clone());
            let app = app(&key.pubkey, &server.url);
            let dir = tempfile::tempdir().expect("tempdir");
            let file = dir.path().join("desktop.json");
            if let Some(stored) = stored {
                std::fs::write(&file, stored).expect("profile");
            }
            let service = Service::build(InstallSource::Direct, Some(file), false, false);
            let view = service.view();
            assert!(view.consent_pending, "{name}: {view:?}");
            assert!(!view.enabled, "{name}: {view:?}");
            assert!(!view.automatic, "{name}: {view:?}");
            assert!(!service.requests_allowed(), "{name}");
            assert_eq!(
                service.set_automatic(app.handle(), true).unwrap_err(),
                "updater_consent_required",
                "{name}"
            );

            tauri::async_runtime::block_on(service.tick(app.handle()));
            let manual = tauri::async_runtime::block_on(service.check_now(app.handle()));
            assert_eq!(
                manual.as_ref().err().map(String::as_str),
                Some("updater_consent_required"),
                "{name}"
            );
            assert_eq!(service.view().phase, "idle", "{name}");
            assert_eq!(server.count(), 0, "{name}");
        }
    }

    #[test]
    fn answering_no_persists_the_choice_and_keeps_every_request_off() {
        let key = keypair();
        let server = Local::http(r#"{"version":"0.2.0","platforms":{}}"#.to_string());
        let app = app(&key.pubkey, &server.url);
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("desktop.json");
        let service = Service::build(InstallSource::Direct, Some(file.clone()), false, false);

        let view = service
            .answer_automatic(app.handle(), false)
            .expect("answer");
        assert!(!view.consent_pending);
        assert!(!view.automatic);
        assert!(!view.enabled);
        assert_eq!(view.disabled_reason, Some("preference"));
        assert_eq!(
            crate::settings::read_updater(&file),
            UpdaterPrefs {
                automatic: false,
                last_check: None,
                consent_answered: true,
            }
        );
        tauri::async_runtime::block_on(service.tick(app.handle()));
        assert_eq!(
            tauri::async_runtime::block_on(service.check_now(app.handle())).unwrap_err(),
            "updater_off"
        );
        assert_eq!(server.count(), 0);
    }

    /// PRD 0018 RF3.2 / RF1.4: desligado, nenhum pedido sai — nem na abertura, nem no
    /// "Verificar agora". O servidor local conta as conexões e precisa ver zero.
    #[test]
    fn a_feature_that_is_off_makes_no_request_at_all() {
        let key = keypair();
        let newer = "0.2.0";
        let manifest = manifest(
            newer,
            &sign(&key, b"payload", newer),
            "http://127.0.0.1:1/app.tar.gz",
        );

        // Desligado pelo usuário, pelo ambiente, pela chave de build ou por um
        // gerenciador de pacotes: em todos os casos, zero pedidos.
        let cases = [
            ("preference", InstallSource::Direct, false, false, false),
            ("env", InstallSource::Direct, true, true, false),
            ("build", InstallSource::Direct, true, false, true),
            ("homebrew", InstallSource::Homebrew, true, false, false),
            ("scoop", InstallSource::Scoop, true, false, false),
            ("winget", InstallSource::Winget, true, false, false),
            (
                "linux_package",
                InstallSource::LinuxPackage,
                true,
                false,
                false,
            ),
        ];
        for (reason, source, automatic, env_off, build_off) in cases {
            let server = Local::http(manifest.clone());
            let app = app(&key.pubkey, &server.url);
            let dir = tempfile::tempdir().expect("tempdir");
            let file = dir.path().join("desktop.json");
            crate::settings::write_updater(
                &file,
                &UpdaterPrefs {
                    automatic,
                    consent_answered: true,
                    ..UpdaterPrefs::default()
                },
            )
            .expect("preferência gravada");

            let service = Service::build(source, Some(file.clone()), env_off, build_off);
            assert_eq!(service.view().disabled_reason, Some(reason), "{reason}");
            assert!(!service.requests_allowed(), "{reason}");

            // O ciclo da tarefa de 24 h e o "Verificar agora" passam pelo mesmo portão.
            tauri::async_runtime::block_on(service.tick(app.handle()));
            let refused = tauri::async_runtime::block_on(service.check_now(app.handle()));
            assert_eq!(
                refused.as_ref().err().map(String::as_str),
                Some("updater_off"),
                "{reason}"
            );
            assert_eq!(service.view().phase, "idle", "{reason}");
            assert!(!service.view().enabled, "{reason}");
            assert_eq!(server.count(), 0, "{reason}");
        }
    }

    /// "Yes" persists the answer, enables the RF2.1 schedule and lets the
    /// same scheduler path make exactly one request.
    #[test]
    fn answering_yes_starts_the_schedule_and_records_the_check_date() {
        let key = keypair();
        let newer = "0.2.0";
        let manifest = manifest(
            newer,
            &sign(&key, b"payload", newer),
            "https://github.com/jmarcon/cuotai/releases/download/v0.2.0/Cuotai.app.tar.gz",
        );
        let server = Local::http(manifest);
        let app = app(&key.pubkey, &server.url);
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("desktop.json");
        let service = Service::build(InstallSource::Direct, Some(file.clone()), false, false);
        assert!(service.view().consent_pending);
        assert!(!service.requests_allowed());
        assert_eq!(server.count(), 0);

        let answered = service
            .answer_automatic(app.handle(), true)
            .expect("answer");
        assert!(!answered.consent_pending);
        assert!(answered.automatic);
        assert!(answered.enabled);
        assert!(service.requests_allowed());

        tauri::async_runtime::block_on(service.tick(app.handle()));
        let view = service.view();
        assert_eq!(view.phase, "available", "{view:?}");
        assert_eq!(view.version.as_deref(), Some(newer));
        assert!(view.notes.is_some());
        assert_eq!(view.channel, "stable");
        assert_eq!(view.current_version, env!("CARGO_PKG_VERSION"));
        assert!(view.can_install);
        assert_eq!(view.upgrade_command, None);
        assert!(!view.dismissed);
        assert_eq!(server.count(), 1);
        // A data da verificação foi gravada em `desktop.json`; o próximo ciclo espera.
        let stored = crate::settings::read_updater(&file);
        assert!(stored.last_check.is_some());
        assert!(stored.automatic);
        assert!(
            !service.check_due(&stored, chrono::Utc::now()),
            "24 horas depois, não agora"
        );
        // "Lembrar depois" esconde o aviso, sem mexer na versão.
        let dismissed = service.dismiss(app.handle());
        assert!(dismissed.dismissed);
        assert_eq!(dismissed.version.as_deref(), Some(newer));
    }

    #[test]
    fn unsafe_artifact_urls_are_refused_before_any_artifact_request() {
        let key = keypair();
        let newer = "0.2.0";
        let artifact = Local::http("artifact bytes".to_string());
        for download in [
            artifact.url.as_str(),
            "https://example.invalid/jmarcon/cuotai/releases/download/v0.2.0/Cuotai.app.tar.gz",
        ] {
            let manifest = manifest(newer, &sign(&key, b"payload", newer), download);
            let manifest_server = Local::http(manifest);
            let app = app(&key.pubkey, &manifest_server.url);
            let service = Service::build(InstallSource::Direct, None, false, false);
            service
                .answer_automatic(app.handle(), true)
                .expect("answer");

            tauri::async_runtime::block_on(service.tick(app.handle()));
            let view = service.view();
            assert_eq!(view.phase, "failed", "{download}: {view:?}");
            assert_eq!(view.error, Some("insecure_endpoint"), "{download}");
            assert_eq!(manifest_server.count(), 1, "{download}");
            assert_eq!(artifact.count(), 0, "{download}");
        }
    }

    #[test]
    fn a_restored_update_is_revalidated_before_download() {
        let key = keypair();
        let newer = "0.2.0";
        let artifact = Local::http("must not be requested".to_string());
        let manifest_server = Local::http(manifest(
            newer,
            &sign(&key, b"payload", newer),
            &artifact.url,
        ));
        let app = app(&key.pubkey, &manifest_server.url);
        // The plugin itself accepts the untrusted URL from the unsigned
        // manifest; seed the state as though it had been retained/restored.
        let update = check(&app).expect("manifest read").expect("newer version");
        let service = Arc::new(Service::build(InstallSource::Direct, None, false, false));
        *service.phase.lock() = Phase::Available {
            version: update.version,
            notes: update.body,
            date: None,
            raw: update.raw_json,
            dismissed: false,
        };

        assert_eq!(
            service.install(app.handle()),
            Err("insecure_endpoint".into())
        );
        assert_eq!(service.view().phase, "available");
        assert_eq!(manifest_server.count(), 1);
        assert_eq!(artifact.count(), 0);
    }

    #[test]
    fn cancelling_restores_the_pending_update_without_another_check() {
        let key = keypair();
        let app = app(&key.pubkey, "http://127.0.0.1:1/latest.json");
        let service = Service::build(InstallSource::Direct, None, false, false);
        let raw = serde_json::json!({"version": "0.2.0", "platforms": {}});
        *service.phase.lock() = Phase::Downloading {
            version: "0.2.0".into(),
            notes: Some("notes".into()),
            date: Some("2026-10-22T12:00:00Z".into()),
            raw: raw.clone(),
            downloaded: 9,
            total: Some(10),
        };

        let view = service.cancel_download(app.handle());
        assert_eq!(view.phase, "available");
        assert_eq!(view.version.as_deref(), Some("0.2.0"));
        assert_eq!(view.notes.as_deref(), Some("notes"));
        assert_eq!(view.date.as_deref(), Some("2026-10-22T12:00:00Z"));
        match &*service.phase.lock() {
            Phase::Available {
                raw: restored,
                dismissed,
                ..
            } => {
                assert_eq!(restored, &raw);
                assert!(!dismissed);
            }
            _ => panic!("cancel must restore the pending update"),
        };
    }

    #[test]
    fn a_login_gate_keeps_the_verified_package_ready() {
        let key = keypair();
        let app = app(&key.pubkey, "http://127.0.0.1:1/latest.json");
        let service = Service::build(InstallSource::Direct, None, false, false);
        let raw = serde_json::json!({"version": "0.2.0", "platforms": {}});
        *service.phase.lock() = Phase::Ready {
            version: "0.2.0".into(),
            raw: raw.clone(),
            bytes: b"verified package".to_vec(),
        };
        let prepared = std::sync::atomic::AtomicBool::new(false);

        assert_eq!(
            service.restart(app.handle(), true, || {
                prepared.store(true, Ordering::SeqCst)
            }),
            Err("login_in_progress".into())
        );
        assert!(!prepared.load(Ordering::SeqCst));
        match &*service.phase.lock() {
            Phase::Ready {
                raw: kept, bytes, ..
            } => {
                assert_eq!(kept, &raw);
                assert_eq!(bytes, b"verified package");
            }
            _ => panic!("the login gate must leave the package ready"),
        };
    }

    /// Um `View` de gerenciador de pacotes traz o comando e diz que não atualiza no lugar.
    #[test]
    fn a_managed_install_offers_the_package_manager_command() {
        let key = keypair();
        let server = Local::http(r#"{"version":"0.2.0","platforms":{}}"#.to_string());
        let app = app(&key.pubkey, &server.url);
        let service = Service::build(InstallSource::Homebrew, None, false, false);
        let view = service.view();
        assert_eq!(view.disabled_reason, Some("homebrew"));
        assert_eq!(view.upgrade_command, Some("brew upgrade --cask cuotai"));
        assert!(!view.can_install);
        assert!(!view.enabled);
        assert!(!view.consent_pending, "managed installs are never asked");
        // Endpoint state never overrides package ownership or opens the request gate.
        tauri::async_runtime::block_on(service.tick(app.handle()));
        assert_eq!(
            tauri::async_runtime::block_on(service.check_now(app.handle())).unwrap_err(),
            "updater_off"
        );
        assert_eq!(service.view().disabled_reason, Some("homebrew"));
        assert_eq!(server.count(), 0);
    }

    /// Um servidor TLS local com certificado autoassinado (RF4.3).
    struct TlsServer {
        listener: TcpListener,
        address: String,
    }

    impl TlsServer {
        fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            let address = format!("https://{}", listener.local_addr().expect("addr"));
            Self { listener, address }
        }

        fn url(&self) -> String {
            format!("{}/latest.json", self.address)
        }

        fn port(&self) -> u16 {
            self.listener.local_addr().expect("addr").port()
        }

        /// Serve `body` uma vez. O handshake falha (o cliente recusa o certificado antes
        /// de mandar qualquer pedido), e a conexão é descartada.
        fn serve(self, body: String) {
            std::thread::spawn(move || {
                let (socket, _) = match self.listener.accept() {
                    Ok(accepted) => accepted,
                    Err(_) => return,
                };
                let generated =
                    match rcgen::generate_simple_self_signed(vec!["localhost".to_string()]) {
                        Ok(generated) => generated,
                        Err(_) => return,
                    };
                let config = rustls::ServerConfig::builder()
                    .with_no_client_auth()
                    .with_single_cert(
                        vec![generated.cert.der().clone()],
                        rustls::pki_types::PrivateKeyDer::Pkcs8(
                            generated.key_pair.serialize_der().into(),
                        ),
                    );
                let Ok(config) = config else { return };
                let connection = match rustls::ServerConnection::new(Arc::new(config)) {
                    Ok(connection) => connection,
                    Err(_) => return,
                };
                let _ = socket.set_read_timeout(Some(Duration::from_secs(5)));
                let mut tls = rustls::StreamOwned::new(connection, socket);
                let mut request = Vec::new();
                let mut chunk = [0u8; 1024];
                // Ler pelo `StreamOwned` dirige o handshake e apresenta de
                // verdade o certificado autoassinado ao cliente.
                while let Ok(read) = tls.read(&mut chunk) {
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..read]);
                    if request.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let _ = write!(
                    tls,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
            });
        }
    }
}
