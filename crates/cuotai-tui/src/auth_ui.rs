//! Diálogo de login da TUI. Toda a política e orquestração de login vivem no
//! core (`cuotai_core::login`, RF2); este módulo apenas renderiza os eventos
//! tipados e encaminha o código colado pelo usuário.

use crate::ui::safe;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use cuotai_core::{
    login::{
        CleanupFailure, CompletedLogin, CredentialCommit, ExternalLogin, LoginCommitOutcome,
        LoginDriver, LoginEvent, LoginResult, PartialFailure, SubmitCodeError,
    },
    oauth::{OAuthError, OAuthSession},
    Config,
};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use std::sync::Arc;
use tokio::task::JoinHandle;

// Paleta Dracula local — mesma do dashboard/wizard.
const DRACULA_BG: Color = Color::Rgb(40, 42, 54);
const DRACULA_FG: Color = Color::Rgb(248, 248, 242);
const DRACULA_CYAN: Color = Color::Rgb(139, 233, 253);

pub(crate) enum Outcome {
    /// Cuotai credential authorized; the token is memory-only until commit.
    Authorized(CompletedLogin),
    /// External CLI completed; only public account params are returned.
    External(ExternalLogin),
    /// Credencial gravada no cofre; falta apenas a gravação do TOML.
    CredentialReady(CredentialCommit),
    /// TOML saved and post-save cleanup resolved (possibly non-fatally).
    Committed(LoginCommitOutcome),
    /// A gravação do TOML falhou após a credencial ser gravada; a
    /// compensação do core já foi aplicada e o estado exato é relatado.
    RolledBack(PartialFailure),
}

enum Job {
    /// Fluxo interativo conduzido pelo driver do core.
    Login(LoginDriver),
    /// Etapa 1 do commit (cofre). Não cancelável.
    Commit(JoinHandle<Result<CredentialCommit, OAuthError>>),
    /// TOML já salvo; limpeza da chave substituída em andamento.
    Confirm(JoinHandle<LoginCommitOutcome>),
    /// Compensação após falha da gravação do TOML; também não cancelável.
    Rollback(JoinHandle<PartialFailure>),
    /// Resultado já coletado.
    Done,
}

pub(crate) struct LoginTask {
    job: Job,
    lines: Vec<String>,
    scroll: u16,
    code: String,
    code_status: String,
}
impl LoginTask {
    /// Envolve um `LoginDriver` já iniciado pelo core
    /// (`cuotai_core::login::start_login`).
    pub fn new(driver: LoginDriver) -> Self {
        Self {
            job: Job::Login(driver),
            lines: Vec::new(),
            scroll: 0,
            code: String::new(),
            code_status: "Cole o código desta tentativa e pressione Enter.".into(),
        }
    }

    /// Etapa 1 do commit RF2.4: captura o token anterior (reauth) e grava a
    /// credencial no cofre. O TOML é gravado depois pelo chamador, seguido de
    /// `CredentialCommit::confirm` ou `LoginTask::rollback`.
    pub fn begin_commit(session: Arc<OAuthSession>, login: CompletedLogin) -> Self {
        let job = tokio::spawn(async move {
            cuotai_core::login::begin_credential_commit(&session, &login).await
        });
        Self {
            job: Job::Commit(job),
            lines: vec!["Gravando credenciais no cofre do sistema. Autorize o acesso ao Keychain, se solicitado.".into()],
            scroll: 0,
            code: String::new(),
            code_status: String::new(),
        }
    }

    /// Finalize after the TOML save: shared-key guard and replaced-key
    /// cleanup run only now, never before the config is durable.
    pub fn confirm(
        session: Arc<OAuthSession>,
        commit: CredentialCommit,
        saved_config: Config,
    ) -> Self {
        let job = tokio::spawn(async move { commit.confirm(&session, &saved_config).await });
        Self {
            job: Job::Confirm(job),
            lines: vec!["Configuração salva; verificando a credencial substituída...".into()],
            scroll: 0,
            code: String::new(),
            code_status: String::new(),
        }
    }
    /// Compensação do commit quando a gravação do TOML falhou: o core
    /// restaura o token anterior (reauth) ou remove o token novo, e devolve
    /// o estado exato para a UI.
    pub fn rollback(session: Arc<OAuthSession>, commit: CredentialCommit, message: String) -> Self {
        let job = tokio::spawn(async move { commit.rollback(&session, message).await });
        Self {
            job: Job::Rollback(job),
            lines: vec![
                "A configuração não pôde ser salva; desfazendo a gravação no cofre...".into(),
            ],
            scroll: 0,
            code: String::new(),
            code_status: String::new(),
        }
    }

    pub fn is_finished(&self) -> bool {
        match &self.job {
            Job::Login(driver) => driver.is_finished(),
            Job::Commit(job) => job.is_finished(),
            Job::Confirm(job) => job.is_finished(),
            Job::Rollback(job) => job.is_finished(),
            Job::Done => true,
        }
    }

    /// Drena os eventos tipados do core para as linhas do diálogo.
    pub fn drain(&mut self) -> bool {
        let Job::Login(driver) = &mut self.job else {
            return false;
        };
        let mut changed = false;
        while let Some(event) = driver.try_next_event() {
            changed = true;
            match event {
                LoginEvent::Prompt {
                    url,
                    user_code,
                    instructions,
                } => {
                    self.lines = vec!["Autorize no navegador do sistema:".into(), url];
                    if let Some(code) = user_code {
                        self.lines.push(format!("Código de autorização: {code}"));
                    }
                    self.lines.push(instructions);
                }
                LoginEvent::Progress { text } => self.lines.push(text),
                LoginEvent::WaitingCode => {
                    self.code_status = "Cole o código desta tentativa e pressione Enter.".into()
                }
                LoginEvent::CodeRejected { message } => self.code_status = message,
                LoginEvent::BrowserUnavailable { .. } => self.lines.push(
                    "Não foi possível abrir o navegador; abra a URL acima manualmente.".into(),
                ),
                LoginEvent::Finished { .. } => {}
                LoginEvent::Failed { message } => self.lines.push(message),
            }
        }
        changed
    }

    pub async fn result(&mut self) -> Result<Outcome, OAuthError> {
        match std::mem::replace(&mut self.job, Job::Done) {
            Job::Login(mut driver) => Ok(match driver.finish().await? {
                LoginResult::Authorized(login) => Outcome::Authorized(login),
                LoginResult::External(login) => Outcome::External(login),
            }),
            Job::Commit(job) => Ok(Outcome::CredentialReady(
                job.await.map_err(|_| OAuthError::Network)??,
            )),
            Job::Confirm(job) => Ok(Outcome::Committed(
                job.await.map_err(|_| OAuthError::Network)?,
            )),
            Job::Rollback(job) => Ok(Outcome::RolledBack(
                job.await.map_err(|_| OAuthError::Network)?,
            )),
            Job::Done => Err(OAuthError::InvalidResponse),
        }
    }

    pub fn scroll(&mut self, down: bool) {
        self.scroll = if down {
            self.scroll.saturating_add(1)
        } else {
            self.scroll.saturating_sub(1)
        };
    }

    /// Somente fluxos interativos podem ser cancelados (Esc). Durante o
    /// commit/rollback a gravação segue até o fim para nunca deixar o cofre
    /// e o TOML em estado ambíguo.
    pub fn can_cancel(&self) -> bool {
        matches!(self.job, Job::Login(_))
    }

    pub fn accepts_code(&self) -> bool {
        match &self.job {
            Job::Login(driver) => driver.accepts_code(),
            _ => false,
        }
    }

    pub fn paste(&mut self, text: &str) {
        if !self.accepts_code() {
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
        if !self.accepts_code() {
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
                let Job::Login(driver) = &self.job else {
                    return;
                };
                match driver.submit_code(code) {
                    Ok(()) => self.code_status = "Validando código na sessão pendente...".into(),
                    Err(SubmitCodeError::Full(code)) => {
                        self.code = code;
                        self.code_status = "Aguarde a validação anterior antes de reenviar.".into();
                    }
                    Err(SubmitCodeError::Closed) => {
                        self.code_status = "A sessão de login já terminou.".into()
                    }
                    Err(SubmitCodeError::Unsupported) => {}
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
            Constraint::Length(if self.accepts_code() { 6 } else { 3 }),
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
        let footer = if self.accepts_code() {
            format!("Código MiMo (oculto): {} [{} caracteres]\n{}\nEnter envia • Ctrl-U limpa • Esc cancela • ↑↓ rolam instruções", "*".repeat(self.code.len().min(32)), self.code.len(), self.code_status)
        } else if matches!(
            self.job,
            Job::Commit(_) | Job::Confirm(_) | Job::Rollback(_)
        ) {
            "Aguarde a gravação segura. Ctrl-C sai; não cancele durante a compensação.".into()
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

/// Texto da falha parcial RF2.4: a UI apenas apresenta o estado exato
/// reportado pelo core, sem inventar política.
pub(crate) fn partial_failure_message(failure: &PartialFailure) -> String {
    use cuotai_core::login::{CommitStage, Compensation};
    let base = format!("Falha ao salvar o login: {}", failure.message);
    match (failure.stage, failure.compensation) {
        (CommitStage::Vault, _) => format!(
            "{base}\n\nNada foi gravado: nem a credencial no cofre, nem a configuração. Tente novamente."
        ),
        (_, Compensation::PreviousRestored) => format!(
            "{base}\n\nA configuração NÃO foi salva e o token anterior foi restaurado no cofre. A conta continua usando o login anterior; corrija o problema e salve de novo."
        ),
        (_, Compensation::NewRemoved) => format!(
            "{base}\n\nA configuração NÃO foi salva e a nova credencial foi removida do cofre. Corrija o problema e tente novamente."
        ),
        (_, Compensation::Failed) => format!(
            "{base}\n\nA configuração NÃO foi salva, mas a credencial permanece gravada no cofre e não foi possível desfazer. Salvar novamente reutiliza essa credencial (mesma chave); se preferir descartar, remova a conta e refaça o login."
        ),
        (_, Compensation::NotNeeded) => format!(
            "{base}\n\nA gravação do TOML foi o que falhou; o cofre não precisava de compensação."
        ),
    }
}

/// Truthful non-fatal outcome: both the new config and credential were saved,
/// but the replaced token could not be removed.
pub(crate) fn cleanup_failure_message(failure: &CleanupFailure) -> String {
    format!(
        "Login salvo com aviso: {}\n\nA conta já usa a nova credencial; a anterior foi mantida no cofre.",
        failure.message
    )
}
