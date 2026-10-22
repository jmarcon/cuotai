//! PRD 0018 — Updates automáticos do app desktop.
//!
//! O shell nunca fala de endereços nem de assinaturas com a interface: este módulo é o
//! único que constrói o `Updater` do plugin oficial (`tauri-plugin-updater`), deixa o
//! plugin verificar a assinatura do pacote baixado e decide o que a janela principal e a
//! bandeja mostram.
//!
//! Privacidade (RF2.1): a verificação só acontece com o recurso ligado — a preferência de
//! `desktop.json`, nenhuma variável de ambiente ou chave de build desligando, e nenhuma
//! instalação gerenciada por um gerenciador de pacotes (RF3.2/RF2.5). O pedido é um GET
//! de um `latest.json` público: a versão atual, o sistema e a arquitetura ficam neste
//! computador, porque o plugin as usa para escolher a entrada `platforms` do manifesto.
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

/// PRD 0018 RF2.5 / PRD 0021 RF8.1: de onde veio esta cópia do app. Um único detector,
/// usado pela política do atualizador (liga/desliga) e pelo guia ao usuário.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallSource {
    /// Download direto (dmg copiado para `/Applications`, zip, tarball): atualiza no lugar.
    Direct,
    /// AppImage (`APPIMAGE` no ambiente): o plugin troca o próprio AppImage.
    AppImage,
    /// macOS: rodando de uma imagem de disco montada — não dá para escrever nela.
    DiskImage,
    /// macOS App Translocation: diretório somente leitura criado pelo Gatekeeper.
    Translocated,
    /// O local da instalação não aceita escrita.
    ReadOnly,
    /// Homebrew (`<prefix>/Caskroom/cuotai`).
    Homebrew,
    /// Scoop (`...\scoop\apps\cuotai`).
    Scoop,
    /// winget (`...\Microsoft\WinGet\Packages\...`). Uma instalação feita pelo instalador
    /// do sistema é indistinguível de uma manual: quem desliga é a chave de build.
    Winget,
    /// Pacote da distribuição ou AUR em `/usr/...` (deb, rpm).
    LinuxPackage,
}

impl InstallSource {
    /// Um gerenciador de pacotes é dono do arquivo: o atualizador interno não consulta
    /// nem instala (RF3.2/RF2.5) e a interface mostra o comando do gerenciador.
    pub fn managed(self) -> bool {
        matches!(
            self,
            InstallSource::Homebrew
                | InstallSource::Scoop
                | InstallSource::Winget
                | InstallSource::LinuxPackage
        )
    }

    /// Só estas duas cópias podem ser atualizadas no lugar (RF2.5).
    pub fn can_install(self) -> bool {
        matches!(self, InstallSource::Direct | InstallSource::AppImage)
    }

    /// Código estável do motivo, traduzido pela interface.
    pub fn reason(self) -> &'static str {
        match self {
            InstallSource::Homebrew => "homebrew",
            InstallSource::Scoop => "scoop",
            InstallSource::Winget => "winget",
            InstallSource::LinuxPackage => "linux_package",
            _ => "other",
        }
    }

    /// O comando que atualiza esta instalação (PRD 0021 RF8.1). deb, rpm e AUR usam o
    /// gerenciador da distribuição; o app não adivinha qual é, e o AUR pede revisão.
    pub fn upgrade_command(self) -> Option<&'static str> {
        match self {
            InstallSource::Homebrew => Some("brew upgrade --cask cuotai"),
            InstallSource::Scoop => Some("scoop update cuotai"),
            InstallSource::Winget => Some("winget upgrade Cuotai.Cuotai"),
            InstallSource::LinuxPackage => Some("sudo apt upgrade cuotai"),
            _ => None,
        }
    }
}

/// Detecta a origem da instalação a partir do caminho do executável, do `APPIMAGE` e de
/// saber se a pasta aceita escrita. Função pura: os testes passam pastas de exemplo.
pub fn detect_in(exe: &Path, appimage: Option<&Path>, writable: bool) -> InstallSource {
    if appimage.is_some() {
        return InstallSource::AppImage;
    }
    let path = exe.to_string_lossy().replace('\\', "/");
    // macOS: `/Volumes/<disco>/Cuotai.app/...` é a imagem de disco montada e a App
    // Translocation é um diretório temporário somente leitura. Nenhuma das duas se
    // atualiza no lugar, mesmo quando o caminho parece gravável.
    if path.starts_with("/Volumes/") {
        return InstallSource::DiskImage;
    }
    if path.contains("/AppTranslocation/") {
        return InstallSource::Translocated;
    }
    // Gerenciadores de pacotes (PRD 0021 RF8.1).
    if path.contains("/Caskroom/cuotai/") {
        return InstallSource::Homebrew;
    }
    if path.contains("/scoop/apps/") {
        return InstallSource::Scoop;
    }
    if path
        .to_lowercase()
        .contains("/appdata/local/microsoft/winget/packages/")
    {
        return InstallSource::Winget;
    }
    if path.starts_with("/usr/bin/")
        || path.starts_with("/usr/sbin/")
        || path.starts_with("/usr/lib/")
    {
        return InstallSource::LinuxPackage;
    }
    if !writable {
        return InstallSource::ReadOnly;
    }
    InstallSource::Direct
}

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
    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
    let writable = install_dir(&exe).is_none_or(|dir| dir_is_writable(&dir));
    detect_in(&exe, appimage.as_deref(), writable)
}

/// PRD 0018 RF3.2/RF2.5: por que o atualizador está desligado agora, nesta ordem: o
/// ambiente vence a chave de build, que vence o gerenciador de pacotes, e desligar pelo
/// usuário é o último motivo. `None` = pode verificar.
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
    if source.managed() {
        return Some(source.reason());
    }
    if !prefs.automatic {
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

/// Estado interno do atualizador. O `Update` do plugin não fica guardado: o manifesto
/// cru (`raw`) permite refazê-lo a cada tentativa — inclusive depois de um cancelamento —
/// sem uma segunda verificação na rede.
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
        downloaded: u64,
        total: Option<u64>,
    },
    /// Baixado, verificado e instalado; falta reiniciar (RF2.4).
    Installing {
        version: String,
    },
    Ready {
        version: String,
    },
    Failed {
        code: &'static str,
    },
}

/// PRD 0018 RF2.2/RF3.1: o que a interface mostra, sem URL, assinatura ou chave.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct UpdateView {
    /// O app pode verificar agora (RF3.2).
    pub enabled: bool,
    /// A preferência do usuário (o interruptor de Settings → Updates).
    pub automatic: bool,
    /// Por que está desligado, quando está.
    pub disabled_reason: Option<&'static str>,
    pub channel: &'static str,
    pub current_version: String,
    pub last_check: Option<String>,
    /// Primeira execução: a explicação do que o pedido envia ainda não foi aceita.
    pub explanation_pending: bool,
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
            env_off,
            build_off,
            stopping: Arc::new(AtomicBool::new(false)),
        }
    }

    fn prefs(&self) -> UpdaterPrefs {
        self.prefs_file
            .as_deref()
            .map(settings::read_updater)
            .unwrap_or_default()
    }

    fn save_prefs(&self, prefs: &UpdaterPrefs) -> Result<(), &'static str> {
        match &self.prefs_file {
            Some(file) => settings::write_updater(file, prefs),
            None => Ok(()),
        }
    }

    /// Por que o atualizador está desligado agora (RF3.2/RF2.5).
    pub fn disabled_reason(&self) -> Option<&'static str> {
        disabled_reason(&self.prefs(), self.env_off, self.build_off, self.source)
    }

    /// `true` quando uma verificação pode ser feita; desligado, nenhum pedido sai.
    pub fn requests_allowed(&self) -> bool {
        self.disabled_reason().is_none()
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
        let reason = self.disabled_reason();
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
            Phase::Ready { version } => (
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
            enabled: reason.is_none(),
            automatic: prefs.automatic,
            disabled_reason: reason,
            channel: CHANNEL,
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            last_check: prefs.last_check.clone(),
            explanation_pending: reason.is_none() && !prefs.explanation_seen,
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

    /// RF2.1: liga/desliga a verificação automática (`desktop.json`). Desligar impede
    /// qualquer pedido, inclusive o "Verificar agora" (RF3.2). Erro: `unwritable`.
    pub fn set_automatic(&self, app: &AppHandle, on: bool) -> Result<UpdateView, String> {
        let mut prefs = self.prefs();
        prefs.automatic = on;
        self.save_prefs(&prefs).map_err(str::to_owned)?;
        self.emit(app);
        self.nudge.notify_one();
        Ok(self.view())
    }

    /// RF2.1: a explicação do que a verificação envia foi aceita e não volta.
    pub fn explanation_seen(&self, app: &AppHandle) -> Result<UpdateView, String> {
        let mut prefs = self.prefs();
        prefs.explanation_seen = true;
        self.save_prefs(&prefs).map_err(str::to_owned)?;
        self.emit(app);
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

    /// RF3.1: "Verificar agora". Desligado, responde `updater_off` sem fazer pedido.
    pub async fn check_now<R: Runtime>(&self, app: &AppHandle<R>) -> Result<UpdateView, String> {
        if !self.requests_allowed() {
            return Err("updater_off".to_string());
        }
        self.check(app).await;
        Ok(self.view())
    }

    /// RF2.1: um ciclo da tarefa de 24 h — verifica só quando é devida e o recurso está
    /// ligado; desligado, nenhum pedido sai (RF3.2).
    pub(crate) async fn tick<R: Runtime>(&self, app: &AppHandle<R>) {
        if !self.requests_allowed() || !self.check_due(&self.prefs(), chrono::Utc::now()) {
            return;
        }
        self.check(app).await;
    }

    /// Uma verificação: marca `checking`, pede o manifesto ao plugin e guarda o
    /// resultado. Nada é baixado aqui (RF2.2).
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
            Ok(Some(update)) => Phase::Available {
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
            },
            Ok(None) => Phase::UpToDate,
            Err(error) => Phase::Failed {
                code: error_code(&error),
            },
        }
    }

    /// RF2.3: "Atualizar agora". Sem este comando nada baixa: baixa com progresso
    /// cancelável, o plugin verifica a assinatura e instala; a versão atual só é trocada
    /// quando a instalação termina.
    pub fn install(self: &Arc<Self>, app: &AppHandle) -> Result<UpdateView, String> {
        let (raw, version) = {
            let phase = self.phase.lock();
            match &*phase {
                Phase::Available { raw, version, .. } => (raw.clone(), version.clone()),
                Phase::Downloading { .. } | Phase::Installing { .. } => {
                    return Err("already_running".to_string())
                }
                _ => return Err("no_update".to_string()),
            }
        };
        let Some(updater) = updater_for(app) else {
            return Err("unavailable".to_string());
        };
        let Ok(Some(update)) = updater.restore_update(raw) else {
            return Err("stale".to_string());
        };
        *self.phase.lock() = Phase::Downloading {
            version: version.clone(),
            downloaded: 0,
            total: None,
        };
        self.emit(app);

        let service = ServiceHandle(Arc::clone(self));
        let app = app.clone();
        let handle = tauri::async_runtime::spawn(async move {
            // `download` verifica a assinatura (RF2.3); `install` só roda com os bytes já
            // conferidos e não é cancelável (é local e rápido: interromper no meio
            // deixaria o app pela metade).
            match update
                .download(|chunk, total| service.chunk(&app, chunk, total), || {})
                .await
            {
                Err(error) => service.fail(&app, error_code(&error)),
                Ok(bytes) => {
                    service.set_phase(
                        &app,
                        Phase::Installing {
                            version: version.clone(),
                        },
                    );
                    match update.install(&bytes) {
                        Ok(()) => service.set_phase(&app, Phase::Ready { version }),
                        Err(error) => service.fail(&app, error_code(&error)),
                    }
                }
            }
            *service.0.download.lock() = None;
        });
        *self.download.lock() = Some(handle);
        Ok(self.view())
    }

    /// RF2.3: cancela o download em curso; a versão instalada continua intacta.
    pub fn cancel_download(&self, app: &AppHandle) -> UpdateView {
        if let Some(handle) = self.download.lock().take() {
            handle.abort();
        }
        let mut phase = self.phase.lock();
        if let Phase::Downloading { .. } = &*phase {
            // A versão anunciada continua válida (o manifesto cru sobrevive): o aviso
            // volta e "Atualizar agora" pode tentar de novo sem nova verificação.
            *phase = Phase::Idle;
        }
        drop(phase);
        self.emit(app);
        self.view()
    }

    /// RF2.4: reinicia para terminar a atualização. `prepare` é o gancho do backend
    /// (interromper o ciclo de atualização com cuidado e persistir o estado) e
    /// `login_in_progress` bloqueia com `login_in_progress` enquanto houver um login.
    pub fn restart(
        &self,
        app: &AppHandle,
        login_in_progress: bool,
        prepare: impl FnOnce(),
    ) -> Result<(), String> {
        if login_in_progress {
            return Err("login_in_progress".to_string());
        }
        if !matches!(&*self.phase.lock(), Phase::Ready { .. }) {
            return Err("no_update".to_string());
        }
        prepare();
        self.stopping.store(true, Ordering::Relaxed);
        app.cleanup_before_exit();
        app.restart();
    }

    /// PRD 0018 RF2.1: verifica na abertura e depois uma vez a cada 24 h, enquanto o
    /// recurso estiver ligado. A espera termina na próxima verificação ou num aviso
    /// ("Verificar agora" verifica direto, sem esperar a tarefa).
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

    /// Quanto falta para a próxima verificação automática (nunca menos que um instante).
    fn until_next_check(&self) -> Duration {
        let now = chrono::Utc::now();
        let prefs = self.prefs();
        // Desligado, esta tarefa não marca o tempo do pedido: reavalia de hora em hora
        // (uma mudança de preferência também a acorda).
        if !self.requests_allowed() {
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
    fn chunk(&self, app: &AppHandle, chunk: usize, total: Option<u64>) {
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

    fn set_phase(&self, app: &AppHandle, next: Phase) {
        *self.0.phase.lock() = next;
        self.0.emit(app);
    }

    fn fail(&self, app: &AppHandle, code: &'static str) {
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
    use super::{detect_in, disabled_reason, InstallSource, UpdaterPrefs};
    use std::path::PathBuf;

    fn exe(path: &str) -> PathBuf {
        PathBuf::from(path)
    }

    #[test]
    fn direct_and_appimage_installs_update_in_place() {
        assert_eq!(
            detect_in(
                &exe("/Applications/Cuotai.app/Contents/MacOS/Cuotai"),
                None,
                true
            ),
            InstallSource::Direct
        );
        assert_eq!(
            detect_in(
                &exe("/usr/local/bin/Cuotai"),
                Some(&exe("/tmp/Cuotai-x86_64.AppImage")),
                true
            ),
            InstallSource::AppImage
        );
    }

    #[test]
    fn macos_cannot_update_where_it_runs_from() {
        // Uma imagem de disco montada é lida, não escrita: `writable` não muda o motivo.
        assert_eq!(
            detect_in(
                &exe("/Volumes/Cuotai/Cuotai.app/Contents/MacOS/Cuotai"),
                None,
                false
            ),
            InstallSource::DiskImage
        );
        assert_eq!(
            detect_in(
                &exe("/Volumes/Cuotai/Cuotai.app/Contents/MacOS/Cuotai"),
                None,
                true
            ),
            InstallSource::DiskImage
        );
        assert_eq!(
            detect_in(
                &exe("/private/var/folders/ab/AppTranslocation/1234/d/Cuotai.app/Contents/MacOS/Cuotai"),
                None,
                true
            ),
            InstallSource::Translocated
        );
    }

    #[test]
    fn a_place_that_cannot_be_written_is_not_a_direct_install() {
        assert_eq!(
            detect_in(&exe("/opt/Cuotai/Cuotai"), None, false),
            InstallSource::ReadOnly
        );
        assert_eq!(
            detect_in(&exe("/opt/Cuotai/Cuotai"), None, true),
            InstallSource::Direct
        );
    }

    #[test]
    fn package_managers_own_their_channels_and_their_command() {
        let cases = [
            (
                "/opt/homebrew/Caskroom/cuotai/0.71.1/Cuotai.app/Contents/MacOS/Cuotai",
                InstallSource::Homebrew,
                "brew upgrade --cask cuotai",
            ),
            (
                "/usr/local/Caskroom/cuotai/0.71.1/Cuotai.app/Contents/MacOS/Cuotai",
                InstallSource::Homebrew,
                "brew upgrade --cask cuotai",
            ),
            (
                "C:/Users/demo/scoop/apps/cuotai/current/Cuotai.exe",
                InstallSource::Scoop,
                "scoop update cuotai",
            ),
            (
                "C:/Users/demo/AppData/Local/Microsoft/WinGet/Packages/Cuotai.Cuotai_MZ394459P08D4Z/Cuotai.exe",
                InstallSource::Winget,
                "winget upgrade Cuotai.Cuotai",
            ),
            ("/usr/bin/cuotai", InstallSource::LinuxPackage, "sudo apt upgrade cuotai"),
            ("/usr/lib/cuotai/cuotai", InstallSource::LinuxPackage, "sudo apt upgrade cuotai"),
        ];
        for (path, expected, command) in cases {
            let source = detect_in(&exe(path), None, true);
            assert_eq!(source, expected, "{path}");
            assert!(source.managed(), "{path}");
            assert!(!source.can_install(), "{path}");
            assert_eq!(source.upgrade_command(), Some(command), "{path}");
            assert_ne!(source.reason(), "other", "{path}");
        }
    }

    #[test]
    fn a_look_alike_path_outside_the_known_roots_is_not_a_manager() {
        // `apps-really`, outro cask e uma pasta `/usr/bin` fora da raiz não são gerenciadores.
        for path in [
            "/home/demo/scoop/apps-really/cuotai/Cuotai",
            "/home/demo/Caskroom/not-cuotai/Cuotai.app/Contents/MacOS/Cuotai",
            "/home/demo/usr/bin/cuotai",
            // O prefixo do WinGet só vale dentro da pasta que o winget cria (`Packages`
            // depois de `Microsoft/WinGet`), não em qualquer caminho que os contenha.
            "/opt/Cuotai/Microsoft/WinGet/Packages/cuotai.exe",
        ] {
            assert_eq!(
                detect_in(&exe(path), None, true),
                InstallSource::Direct,
                "{path}"
            );
        }
    }

    fn prefs(automatic: bool) -> UpdaterPrefs {
        UpdaterPrefs {
            automatic,
            ..UpdaterPrefs::default()
        }
    }

    #[test]
    fn every_reason_stops_every_request() {
        // Ordem: ambiente, chave de build, gerenciador, preferência do usuário.
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
        // Uma cópia de imagem de disco não é gerenciada: pode verificar e explicar o guia.
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
    use super::{validate_endpoint, Url};

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
            Err(error) => assert!(
                matches!(error, tauri_plugin_updater::Error::Reqwest(_)),
                "{error}"
            ),
        }
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
                    ..UpdaterPrefs::default()
                },
            )
            .expect("preferência gravada");

            let service = Service::build(source, Some(file.clone()), env_off, build_off);
            assert_eq!(service.disabled_reason(), Some(reason), "{reason}");
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

    /// Com o recurso ligado, o mesmo caminho faz o pedido e guarda a versão nova; o
    /// ciclo de 24 h grava a data da verificação (RF2.1/RF3.1).
    #[test]
    fn the_connected_path_checks_once_and_records_the_date() {
        let key = keypair();
        let newer = "0.2.0";
        let manifest = manifest(
            newer,
            &sign(&key, b"payload", newer),
            "http://127.0.0.1:1/app.tar.gz",
        );
        let server = Local::http(manifest);
        let app = app(&key.pubkey, &server.url);
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("desktop.json");
        let service = Service::build(InstallSource::Direct, Some(file.clone()), false, false);
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
        // Um erro de endpoint não muda a política da instalação.
        tauri::async_runtime::block_on(service.tick(app.handle()));
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
                let mut tls = rustls::StreamOwned::new(connection, socket);
                let _ = tls.sock.set_read_timeout(Some(Duration::from_secs(5)));
                let mut request = Vec::new();
                let mut chunk = [0u8; 1024];
                // O handshake falha aqui (certificado recusado): o que sobrar é ruído.
                while let Ok(read) = tls.sock.read(&mut chunk) {
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..read]);
                    if request.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let _ = write!(
                    tls.sock,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
            });
        }
    }
}
