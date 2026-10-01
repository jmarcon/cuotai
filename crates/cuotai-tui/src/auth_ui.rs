use crate::ui::safe;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use cuotai_core::{
    oauth::{AuthorizedLogin, LoginPrompt, OAuthConfig, OAuthError, OAuthSession},
    Provider,
};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use std::sync::Arc;
use tokio::{sync::mpsc, task::JoinHandle};

// Paleta Dracula local — mesma do dashboard/wizard.
const DRACULA_BG: Color = Color::Rgb(40, 42, 54);
const DRACULA_FG: Color = Color::Rgb(248, 248, 242);
const DRACULA_CYAN: Color = Color::Rgb(139, 233, 253);

pub(crate) enum Outcome {
    Authorized {
        provider: Provider,
        config: OAuthConfig,
        login: Arc<AuthorizedLogin>,
    },
    Aws,
    Stored,
    /// `bl auth login --console` concluiu no perfil dedicado do próprio CLI;
    /// a consulta de quota usa esse perfil sem copiar credenciais para o Cuotai.
    BailianConsole,
}
enum Event {
    Prompt(LoginPrompt),
    Text(String),
    CodeError(String),
}

pub(crate) struct LoginTask {
    job: Option<JoinHandle<Result<Outcome, OAuthError>>>,
    events: mpsc::Receiver<Event>,
    lines: Vec<String>,
    scroll: u16,
    saving: bool,
    code_sender: Option<mpsc::Sender<String>>,
    code: String,
    code_status: String,
}
impl Drop for LoginTask {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.abort();
        }
    }
}
impl LoginTask {
    pub fn oauth(session: Arc<OAuthSession>, provider: Provider, config: OAuthConfig) -> Self {
        let (tx, events) = mpsc::channel(16);
        let (code_sender, codes) = mpsc::channel(1);
        let job = tokio::spawn(async move {
            if provider == Provider::Cursor {
                let login = Arc::new(session.import_cursor(&config).await?);
                return Ok(Outcome::Authorized {
                    provider,
                    config,
                    login,
                });
            }
            let pending = session.begin(provider, &config).await?;
            let prompt = pending.prompt().clone();
            let url = prompt.url.clone();
            let _ = tx.send(Event::Prompt(prompt)).await;
            if !open_browser(&url).await {
                let _ = tx
                    .send(Event::Text(
                        "Não foi possível abrir o navegador; abra a URL acima manualmente.".into(),
                    ))
                    .await;
            }
            let login = if provider == Provider::XiaomiMimo {
                let (error_tx, mut error_rx) = mpsc::channel(4);
                let finish = pending.finish_with_codes(codes, error_tx);
                tokio::pin!(finish);
                loop {
                    tokio::select! {
                        result = &mut finish => break Arc::new(result?),
                        Some(error) = error_rx.recv() => { let _ = tx.send(Event::CodeError(error)).await; }
                    }
                }
            } else {
                Arc::new(pending.finish().await?)
            };
            Ok(Outcome::Authorized {
                provider,
                config,
                login,
            })
        });
        Self {
            job: Some(job),
            events,
            lines: vec![if provider == Provider::Cursor {
                "Lendo sessão Cursor autorizada, em modo somente leitura...".into()
            } else {
                "Preparando autorização no provedor...".into()
            }],
            scroll: 0,
            saving: false,
            code_sender: (provider == Provider::XiaomiMimo).then_some(code_sender),
            code: String::new(),
            code_status: "Cole o código desta tentativa e pressione Enter.".into(),
        }
    }

    pub fn aws(profile: String) -> Self {
        let (tx, events) = mpsc::channel(16);
        let job = tokio::spawn(async move {
            let (lines_tx, mut lines_rx) = mpsc::channel(16);
            let login = cuotai_core::oauth::aws::login(&profile, lines_tx);
            tokio::pin!(login);
            loop {
                tokio::select! {
                    result = &mut login => { result?; return Ok(Outcome::Aws); }
                    Some(line) = lines_rx.recv() => { let _ = tx.send(Event::Text(line)).await; }
                }
            }
        });
        Self {
            job: Some(job),
            events,
            lines: vec!["AWS SSO: abra a URL e use o código indicados pela AWS CLI.".into()],
            scroll: 0,
            saving: false,
            code_sender: None,
            code: String::new(),
            code_status: String::new(),
        }
    }

    /// Lança `bl auth login --console --console-site <site> --config <profile>`
    /// em subprocesso isolado por perfil. stdout/stderr são descartados
    /// (a CLI pode imprimir chaves/cookies). `kill_on_drop(true)` cancela o
    /// processo se a tarefa for droppada. O login persiste no cofre local do
    /// `bl` no perfil dedicado; cancelar o Cuotai NÃO desfaz.
    pub fn bailian_console(console_site: String, profile: String) -> Self {
        if let Err(error) = validate_console_site(&console_site) {
            return Self::failed(error);
        }
        if let Err(error) = validate_cli_profile(&profile) {
            return Self::failed(error);
        }
        let heading =
            format!("Alibaba Token Plan (Qwen Cloud) — site: {console_site}, perfil: {profile}");
        let (tx, events) = mpsc::channel(8);
        let profile_for_task = profile.clone();
        let job = tokio::spawn(async move {
            // Mensagens estáticas e genéricas — nunca conteúdo do stdout do CLI.
            let _ = tx
                .send(Event::Text(
                    "Iniciando `bl auth login --console`. O navegador pode abrir automaticamente."
                        .into(),
                ))
                .await;
            match bailian_console_inner(&console_site, &profile_for_task).await {
                Ok(()) => {
                    let _ = tx
                        .send(Event::Text(
                            "Login do `bl` concluído no perfil dedicado. Cuotai NÃO guarda nada disto."
                                .into(),
                        ))
                        .await;
                    let _ = tx
                        .send(Event::Text(
                            "A consulta Token Plan usará esse perfil via `bl --config <name>`. `params.session_env` permanece um override opcional (Cookie manual) caso você prefira a rota antiga."
                                .into(),
                        ))
                        .await;
                    Ok(Outcome::BailianConsole)
                }
                Err(error) => {
                    let _ = tx.send(Event::Text(error.to_string())).await;
                    Err(error)
                }
            }
        });
        let mut lines = vec![heading];
        lines.push(
            "`bl auth login --console --console-site <site> --config <profile>` será executado; cancele com Esc."
                .into(),
        );
        lines.push("Cancelar aqui NÃO desfaz o que o `bl` gravou no cofre local.".into());
        lines.push(
            "`bl --console` pode criar uma API key comum; a chave da assinatura deve existir separadamente (`--api-key`). `params.session_env` continua sendo override opcional.".into(),
        );
        Self {
            job: Some(job),
            events,
            lines,
            scroll: 0,
            saving: false,
            code_sender: None,
            code: String::new(),
            code_status: String::new(),
        }
    }

    /// Constrói um LoginTask já concluído com erro, usado quando a validação
    /// de argumentos falha antes mesmo do subprocesso existir. Exige um
    /// runtime tokio em escopo (#[tokio::test] ou o runtime do TUI).
    fn failed(error: OAuthError) -> Self {
        let (_, events) = mpsc::channel(1);
        let job = tokio::spawn(async move { Err::<Outcome, _>(error) });
        Self {
            job: Some(job),
            events,
            lines: vec!["Falha ao iniciar o login `bl`.".into()],
            scroll: 0,
            saving: false,
            code_sender: None,
            code: String::new(),
            code_status: String::new(),
        }
    }

    pub fn store(session: Arc<OAuthSession>, login: Arc<AuthorizedLogin>) -> Self {
        let (_, events) = mpsc::channel(1);
        let job = tokio::spawn(async move {
            session.save_login(&login).await?;
            Ok(Outcome::Stored)
        });
        Self { job: Some(job), events, lines: vec!["Gravando credenciais no cofre do sistema. Autorize o acesso ao Keychain, se solicitado.".into()], scroll: 0, saving: true, code_sender: None, code: String::new(), code_status: String::new() }
    }

    pub fn is_finished(&self) -> bool {
        self.job.as_ref().is_some_and(|j| j.is_finished())
    }
    pub fn drain(&mut self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.events.try_recv() {
            changed = true;
            match event {
                Event::Prompt(prompt) => {
                    self.lines = vec!["Autorize no navegador do sistema:".into(), prompt.url];
                    if let Some(code) = prompt.user_code {
                        self.lines.push(format!("Código de autorização: {code}"));
                    }
                    self.lines.push(
                        "Aguardando sua autorização. Nenhuma senha deve ser digitada no Cuotai."
                            .into(),
                    );
                }
                Event::Text(text) => self.lines.push(text),
                Event::CodeError(error) => self.code_status = error,
            }
        }
        changed
    }
    pub async fn result(&mut self) -> Result<Outcome, OAuthError> {
        self.job
            .take()
            .ok_or(OAuthError::InvalidResponse)?
            .await
            .map_err(|_| OAuthError::Network)?
    }
    pub fn scroll(&mut self, down: bool) {
        self.scroll = if down {
            self.scroll.saturating_add(1)
        } else {
            self.scroll.saturating_sub(1)
        };
    }
    pub fn can_cancel(&self) -> bool {
        !self.saving
    }
    pub fn paste(&mut self, text: &str) {
        if self.code_sender.is_none() {
            return;
        }
        let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        if self.code.len() + compact.len() > 16 * 1024 {
            self.code_status = "Código muito longo. Ctrl-U limpa o campo.".into();
            return;
        }
        if !compact
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '_' | '-' | '='))
        {
            self.code_status = "Cole somente o código de autenticação mostrado pelo MiMo.".into();
            return;
        }
        self.code.push_str(&compact);
    }
    pub fn input(&mut self, key: KeyEvent) {
        if self.code_sender.is_none() {
            return;
        }
        match key.code {
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.code.clear()
            }
            KeyCode::Backspace => {
                self.code.pop();
            }
            KeyCode::Enter if !self.code.is_empty() => {
                let code = std::mem::take(&mut self.code);
                match self.code_sender.as_ref().expect("checked").try_send(code) {
                    Ok(()) => self.code_status = "Validando código na sessão pendente...".into(),
                    Err(error) => {
                        self.code = error.into_inner();
                        self.code_status = "Aguarde a validação anterior antes de reenviar.".into();
                    }
                }
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.paste(&c.to_string())
            }
            _ => {}
        }
    }
    pub fn draw(&mut self, frame: &mut Frame) {
        // Dracula frame background, mesma paleta do dashboard/wizard.
        frame.render_widget(
            Block::default().style(Style::default().bg(DRACULA_BG).fg(DRACULA_FG)),
            frame.area(),
        );
        let areas = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(if self.code_sender.is_some() { 6 } else { 3 }),
        ])
        .split(frame.area());
        let text = self
            .lines
            .iter()
            .map(|s| safe(s).into_owned())
            .collect::<Vec<_>>()
            .join("\n\n");
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Login OAuth ");
        let inner = block.inner(areas[0]);
        let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
        let max = paragraph
            .line_count(inner.width)
            .saturating_sub(usize::from(inner.height))
            .min(usize::from(u16::MAX)) as u16;
        self.scroll = self.scroll.min(max);
        frame.render_widget(paragraph.scroll((self.scroll, 0)).block(block), areas[0]);
        let footer = if self.code_sender.is_some() {
            format!("Código MiMo (oculto): {} [{} caracteres]\n{}\nEnter envia • Ctrl-U limpa • Esc cancela • ↑↓ rolam instruções", "*".repeat(self.code.len().min(32)), self.code.len(), self.code_status)
        } else if self.saving {
            "Aguarde a gravação segura. Ctrl-C sai; o arquivo TOML ainda não foi alterado.".into()
        } else {
            "↑↓ rolar • Esc cancela e retorna ao wizard • Ctrl-C sai".into()
        };
        frame.render_widget(
            Paragraph::new(footer)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(DRACULA_CYAN)),
            areas[1],
        );
    }
}

pub(crate) async fn open_browser(url: &str) -> bool {
    #[cfg(target_os = "macos")]
    let mut cmd = tokio::process::Command::new("open");
    #[cfg(target_os = "linux")]
    let mut cmd = tokio::process::Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = tokio::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    cmd.arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    matches!(tokio::time::timeout(std::time::Duration::from_secs(5), cmd.status()).await, Ok(Ok(status)) if status.success())
}

/// Tempo máximo para `bl auth login --console`. O `bl` pode abrir navegador e
/// esperar interação humana; 10 minutos é folga suficiente sem prender o TUI.
const BAILIAN_LOGIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Sites suportados pelo `bl --console --console-site` no Token Plan internacional.
/// Refusa qualquer valor fora desta lista para nunca repassar argumentos
/// arbitrários ao subprocesso.
const BAILIAN_ALLOWED_SITES: &[&str] = &["international"];

/// Valida o argumento `--console-site` antes de montar a argv. Sites não
/// conhecidos viram `Configuration` com a lista branca exibida ao usuário.
fn validate_console_site(site: &str) -> Result<(), OAuthError> {
    let trimmed = site.trim();
    if trimmed.is_empty() {
        return Err(OAuthError::Configuration(
            "Site do console em branco; informe `international`.".into(),
        ));
    }
    if !BAILIAN_ALLOWED_SITES.contains(&trimmed) {
        return Err(OAuthError::Configuration(format!(
            "Site de console desconhecido: {trimmed}. Valores aceitos: international."
        )));
    }
    if trimmed.starts_with('-') || trimmed.chars().any(char::is_control) {
        return Err(OAuthError::Configuration(
            "Site do console inválido.".into(),
        ));
    }
    Ok(())
}

/// Valida o nome do perfil dedicado (`--config <name>` do `bl`). Limita a
/// ASCII seguro para que o token nunca seja interpretado como flag e nunca
/// colida com `argv` do CLI: letras, dígitos, hífen e underscore; sem
/// leading `-`; até 32 caracteres. O perfil é SEMPRE passado como um
/// argumento argv separado — nunca concatenado.
fn validate_cli_profile(name: &str) -> Result<(), OAuthError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(OAuthError::Configuration(
            "Perfil do `bl` em branco; impossível iniciar o login isolado.".into(),
        ));
    }
    if trimmed.len() > 32 {
        return Err(OAuthError::Configuration(
            "Perfil do `bl` excede 32 caracteres.".into(),
        ));
    }
    if trimmed.starts_with('-') {
        return Err(OAuthError::Configuration(
            "Perfil do `bl` não pode começar com '-'.".into(),
        ));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(OAuthError::Configuration(
            "Perfil do `bl` aceita apenas letras, dígitos, '-' e '_'.".into(),
        ));
    }
    Ok(())
}

/// Spawn do `bl auth login --console --console-site <site>`.
///
/// Segurança:
/// - stdout e stderr vão para o vazio (nunca lidos pelo Cuotai), porque a CLI
///   pode imprimir chaves, cookies e metadados sensíveis.
/// - argv é uma lista fixa mais o `site` validado; nada de env, nada de shell.
/// - stdin null (a CLI pode abrir o navegador sozinha).
/// - `kill_on_drop(true)` + timeout garante que cancelar o Cuotai mata o processo.
/// - API key do provedor, cookie de quota e credenciais OAuth NUNCA são
///   passados na argv, nem via env, nem via stdin.
async fn bailian_console_inner(console_site: &str, profile: &str) -> Result<(), OAuthError> {
    use std::process::Stdio;
    use tokio::process::Command;

    // argv é uma lista fixa: `auth`, `login`, `--console`, `--console-site`,
    // `<site>`, `--config`, `<profile>` — cada item separado, sem concatenação,
    // sem shell. Nada de API key, cookie, token.
    let mut child = Command::new("bl")
        .args([
            "auth",
            "login",
            "--console",
            "--console-site",
            console_site,
            "--config",
            profile,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => OAuthError::Configuration(
                "`bl` (Bailian CLI) não encontrado. Instale a CLI e tente novamente.".into(),
            ),
            _ => OAuthError::Configuration(
                "Não foi possível iniciar o `bl auth login --console`.".into(),
            ),
        })?;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    let timed = tokio::time::timeout(BAILIAN_LOGIN_TIMEOUT, async {
        let (status, (), ()) =
            tokio::join!(child.wait(), drain_silent(stdout), drain_silent(stderr),);
        status
    })
    .await;

    match timed {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(_)) => Err(OAuthError::Configuration(
            "`bl auth login --console` saiu com erro. Verifique a CLI e tente novamente.".into(),
        )),
        Ok(Err(_)) => Err(OAuthError::Configuration(
            "Falha ao aguardar o `bl`.".into(),
        )),
        Err(_) => {
            // Tempo esgotado: mata o filho antes de devolver erro.
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(OAuthError::Expired)
        }
    }
}

/// Descarta silenciosamente qualquer conteúdo do stream — nada jamais chega
/// ao TUI nem a logs. Mantém o pipe ativo para o filho nunca bloquear.
async fn drain_silent(mut stream: impl tokio::io::AsyncRead + Unpin) {
    let _ = tokio::io::copy(&mut stream, &mut tokio::io::sink()).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuotai_core::oauth::OAuthError;

    #[test]
    fn console_site_accepts_only_known_values() {
        // `international` é o único site atualmente aceito.
        assert!(validate_console_site("international").is_ok());
        // Não repassa argv arbitrária ao subprocesso.
        for bad in [
            "",
            "   ",
            "-rf",
            "--help",
            "cn",
            "china",
            "mainland",
            "with spaces",
            "internat\x00ional",
        ] {
            let err = validate_console_site(bad)
                .err()
                .unwrap_or_else(|| panic!("esperava rejeição para {bad:?}"));
            assert!(
                matches!(err, OAuthError::Configuration(_)),
                "rejeição inesperada para {bad:?}: {err:?}"
            );
        }
    }

    #[tokio::test]
    async fn bailian_console_task_fails_fast_on_invalid_site() {
        let mut task = LoginTask::bailian_console("--evil".into(), "cuotai-test".into());
        assert!(matches!(
            task.job.take().unwrap().await.unwrap(),
            Err(OAuthError::Configuration(_))
        ));
    }

    #[tokio::test]
    async fn bailian_console_task_fails_fast_on_invalid_profile() {
        // Perfis com leading '-' ou caracteres não-ASCII-seguros são
        // rejeitados antes do spawn para nunca colidir com parsing de argv.
        for bad in [
            "-leading-dash",
            "with space",
            "uni\u{0301}code",
            "way-too-long-profile-name-that-exceeds-the-limit",
            "",
            "   ",
        ] {
            let mut task = LoginTask::bailian_console("international".into(), bad.into());
            assert!(matches!(
                task.job.take().unwrap().await.unwrap(),
                Err(OAuthError::Configuration(_))
            ));
        }
    }
}
