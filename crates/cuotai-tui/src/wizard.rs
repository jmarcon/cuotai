//! Assistente passo a passo para adicionar/editar contas sem editar TOML manualmente.
//!
//! O assistente armazena apenas NOMES de variáveis de ambiente (ou o perfil AWS),
//! nunca valores de credenciais: valores presentes no ambiente nunca são exibidos
//! nem gravados; apenas a existência (PRESENTE/AUSENTE, com trim) é exibida.
//! `Save` devolve uma cópia de `Config`; quem grava no disco é o chamador e, em
//! caso de falha, o rascunho permanece intacto para nova tentativa.

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use cuotai_core::oauth::{requires_client_registration, supports_login, OAuthConfig};
use cuotai_core::{Account, Config, Provider};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

/// Split vertical único do assistente, compartilhado entre `draw` e
/// `handle_mouse` para que o hit-test nunca divirja da renderização.
/// Rodapé com 5 linhas: borda + linha de botões clicáveis + dica de teclado.
fn wizard_chunks(area: Rect) -> Rc<[Rect]> {
    Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(4),
        Constraint::Length(2),
        Constraint::Length(5),
    ])
    .split(area)
}

pub(crate) enum Action {
    Continue,
    Cancel,
    Save(Config),
    /// Inicia o fluxo OAuth (navegador/device) com o app do próprio usuário.
    Login {
        provider: Provider,
        config: OAuthConfig,
    },
    /// Executa `aws sso login` para um perfil SSO já configurado.
    AwsLogin {
        profile: String,
    },
    /// Executa `bl auth login --console --console-site <site> --config <profile>`
    /// para o Token Plan internacional. Cuotai não armazena nada do cofre
    /// local do `bl`; o `profile` (params.cli_profile) isola credenciais por
    /// conta via `--config`.
    BailianConsoleLogin {
        console_site: String,
        profile: String,
    },
}

const MIN_INTERVAL: u64 = 30;
const MAX_INTERVAL: u64 = 86_400;

// Paleta Dracula para o assistente; o dashboard expõe a mesma paleta em
// outro ponto de desenho. Mantemos estes `const` aqui para que mudanças
// visuais no wizard não dependam do tema do dashboard.
const DRACULA_BG: Color = Color::Rgb(40, 42, 54);
const DRACULA_FG: Color = Color::Rgb(248, 248, 242);
const DRACULA_CYAN: Color = Color::Rgb(139, 233, 253);
const DRACULA_GREEN: Color = Color::Rgb(80, 250, 123);
const DRACULA_YELLOW: Color = Color::Rgb(241, 250, 140);
const DRACULA_RED: Color = Color::Rgb(255, 85, 85);
const DRACULA_COMMENT: Color = Color::Rgb(98, 114, 164);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Provider,
    Name,
    Credential,
    Params,
    Interval,
    Summary,
}

struct Field {
    key: &'static str,
    label: &'static str,
    help: &'static str,
    required: bool,
    kind: FieldKind,
}

enum FieldKind {
    Text(String),
    Choice {
        options: &'static [(&'static str, &'static str)],
        selected: usize,
        omit_default: bool,
    },
}

impl Field {
    fn text(key: &'static str, label: &'static str, help: &'static str, required: bool) -> Self {
        Self {
            key,
            label,
            help,
            required,
            kind: FieldKind::Text(String::new()),
        }
    }
    fn choice(
        key: &'static str,
        label: &'static str,
        help: &'static str,
        options: &'static [(&'static str, &'static str)],
    ) -> Self {
        Self {
            key,
            label,
            help,
            required: false,
            kind: FieldKind::Choice {
                options,
                selected: 0,
                omit_default: true,
            },
        }
    }
}

pub(crate) struct Wizard {
    base: Config,
    editing: Option<usize>,
    step: Step,
    provider: Provider,
    provider_sel: usize,
    provider_offset: u16,
    name: String,
    credential: String,
    fields: Vec<Field>,
    field_idx: usize,
    interval: String,
    summary_scroll: u16,
    error: Option<String>,
    /// Aviso transitório de sucesso (login OAuth/SSO), exibido até a próxima tecla.
    notice: Option<String>,
    /// Modo OAuth no passo Credencial (só vale quando `supports_login(provider)`).
    oauth_mode: bool,
    oauth_client_id: String,
    oauth_tenant: String,
    oauth_secret_env: String,
    oauth_idx: usize,
    /// Config OAuth da conta em edição, preservada enquanto a identidade não mudar.
    oauth_existing: Option<OAuthConfig>,
    /// Config autorizada por login concluído nesta sessão do assistente.
    oauth_authorized: Option<OAuthConfig>,
    /// credential_id estável entre tentativas do mesmo login; novo após qualquer
    /// alteração de identidade (modo, provedor, client_id, tenant, secret_env).
    oauth_pending_id: Option<String>,
    /// Opções não secretas do fluxo nativo (region/auth_method/start_url),
    /// editadas no passo Credencial ANTES do login e gravadas em OAuthConfig.options.
    native_options: Vec<Field>,
    /// Params públicos devolvidos pelo login concluído nesta sessão
    /// (metadados seguros: user_id, region, product etc. — nunca tokens).
    oauth_login_params: BTreeMap<String, String>,
    /// Cópia mutável dos params atuais durante o passo Credencial, usada
    /// para enriquecer `params.cli_profile` (Bailian) sem voltar para os
    /// params imutáveis do `base`. Reset ao avançar.
    credential_params: Option<BTreeMap<String, String>>,
}

impl Wizard {
    pub(crate) fn new(config: &Config, editing: Option<usize>) -> Self {
        let existing = editing.and_then(|i| config.accounts.get(i));
        let provider = existing.map(|a| a.provider).unwrap_or(Provider::OpenAiApi);
        let provider_sel = Provider::ALL
            .iter()
            .position(|p| *p == provider)
            .unwrap_or(0);
        let name = existing.map(|a| a.name.clone()).unwrap_or_default();
        let credential = match existing {
            Some(a) if a.provider == Provider::AwsBedrock => {
                a.params.get("profile").cloned().unwrap_or_default()
            }
            Some(a) => a
                .credential_env
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| default_env(provider).to_string()),
            None => default_env(provider).to_string(),
        };
        // Pré-carrega o OAuth da conta em edição: nome/params podem ser alterados
        // sem refazer o login; mexer na identidade (client/tenant/secret) invalida.
        let oauth_existing = existing
            .filter(|a| a.provider == provider && supports_login(provider))
            .and_then(|a| a.oauth.clone());
        let oauth_mode = oauth_existing.is_some();
        let oauth_client_id = oauth_existing
            .as_ref()
            .map(|o| o.client_id.clone())
            .unwrap_or_default();
        let oauth_tenant = oauth_existing
            .as_ref()
            .and_then(|o| o.tenant.clone())
            .unwrap_or_default();
        let oauth_secret_env = oauth_existing
            .as_ref()
            .and_then(|o| o.client_secret_env.clone())
            .unwrap_or_default();
        let native_options = native_option_fields(provider, oauth_existing.as_ref());
        Self {
            base: config.clone(),
            editing,
            step: Step::Provider,
            provider,
            provider_sel,
            provider_offset: 0,
            name,
            credential,
            fields: default_fields(provider, existing.filter(|a| a.provider == provider)),
            field_idx: 0,
            // Pré-carrega o intervalo individual desta conta, se existir;
            // campo vazio significa "herdar o global". Ao criar conta nova
            // começa vazio (sem override), para que o global continue valendo.
            interval: existing
                .and_then(|a| a.refresh_seconds)
                .map(|seconds| seconds.to_string())
                .unwrap_or_default(),
            summary_scroll: 0,
            error: None,
            notice: None,
            oauth_mode,
            oauth_client_id,
            oauth_tenant,
            oauth_secret_env,
            oauth_idx: 0,
            oauth_existing,
            oauth_authorized: None,
            oauth_pending_id: None,
            native_options,
            oauth_login_params: BTreeMap::new(),
            credential_params: None,
        }
    }

    pub(crate) fn set_error(&mut self, message: String) {
        self.error = Some(message);
    }

    fn provider_at_sel(&self) -> Provider {
        Provider::ALL[self.provider_sel]
    }

    /// Índice selecionado de um campo Choice, se existir.
    fn choice_selected(&self, key: &str) -> Option<usize> {
        self.fields
            .iter()
            .find(|f| f.key == key)
            .and_then(|f| match &f.kind {
                FieldKind::Choice { selected, .. } => Some(*selected),
                FieldKind::Text(_) => None,
            })
    }

    /// Campos visíveis: `org` do Copilot só aparece no modo org com consulta de
    /// cobrança (na quota pessoal não é usada); campos do produto `cli` do
    /// GeminiAi/Grok (session_env, project_id) só aparecem no opt-in, e os campos
    /// da Management API do Grok (team_id, management_key_env) só fora dele.
    fn visible_fields(&self) -> Vec<usize> {
        let mode_org = self.choice_selected("mode") == Some(1);
        let quota_mode = self.choice_selected("query_mode") == Some(1);
        let cli_product = self.choice_selected("product") == Some(1)
            && matches!(self.provider, Provider::GeminiAi | Provider::Grok);
        self.fields
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                // Login nativo: params preenchidos pelos metadados públicos do login
                // (e opções de autenticação) ficam ocultos para nunca divergir da identidade.
                let editable_kiro_profile = self.provider == Provider::Kiro
                    && f.key == "profile_arn"
                    && !self.oauth_login_params.contains_key("profile_arn");
                if self.native_login()
                    && native_param_keys(self.provider).contains(&f.key)
                    && !editable_kiro_profile
                {
                    return false;
                }
                match f.key {
                    "org" => mode_org && !quota_mode,
                    "mode" | "billing" if self.provider == Provider::Copilot => !quota_mode,
                    "session_env" | "email" | "user_id"
                        if matches!(self.provider, Provider::GeminiAi | Provider::Grok) =>
                    {
                        cli_product
                    }
                    "project_id" if self.provider == Provider::GeminiAi => cli_product,
                    "token_env" if self.provider == Provider::Kimi => {
                        self.choice_selected("product") == Some(1)
                    }
                    "team_id" | "management_key_env" if self.provider == Provider::Grok => {
                        !cli_product
                    }
                    _ => true,
                }
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn current_text_mut(&mut self) -> Option<&mut String> {
        match self.step {
            Step::Name => Some(&mut self.name),
            Step::Credential => {
                if self.oauth_active() {
                    self.oauth_text_mut()
                } else {
                    Some(&mut self.credential)
                }
            }
            Step::Interval => Some(&mut self.interval),
            Step::Params => {
                let visible = self.visible_fields();
                let idx = *visible.get(self.field_idx)?;
                match &mut self.fields[idx].kind {
                    FieldKind::Text(value) => Some(value),
                    FieldKind::Choice { .. } => None,
                }
            }
            _ => None,
        }
    }

    /// Avança um passo; ao sair da seleção de provedor, aplica a troca.
    fn advance(&mut self) {
        match self.step {
            Step::Provider => {
                let selected = self.provider_at_sel();
                if selected != self.provider {
                    // Troca de provedor: parâmetros incompatíveis são descartados e a
                    // credencial volta ao padrão do novo provedor (nunca reutiliza a anterior).
                    self.provider = selected;
                    self.fields = default_fields(selected, None);
                    self.field_idx = 0;
                    self.credential = default_env(selected).to_string();
                    // Troca de provedor invalida qualquer login/metadata OAuth anterior.
                    self.oauth_mode = false;
                    self.oauth_client_id.clear();
                    self.oauth_tenant.clear();
                    self.oauth_secret_env.clear();
                    self.oauth_idx = 0;
                    self.invalidate_oauth();
                    self.native_options = native_option_fields(selected, None);
                }
                self.step = Step::Name;
            }
            Step::Name => self.step = Step::Credential,
            Step::Credential => {
                self.field_idx = 0;
                self.step = Step::Params;
            }
            Step::Params => {
                if self.field_idx + 1 < self.visible_fields().len() {
                    self.field_idx += 1;
                } else {
                    self.step = Step::Interval;
                }
            }
            Step::Interval => self.step = Step::Summary,
            Step::Summary => {}
        }
    }

    fn back(&mut self) {
        match self.step {
            Step::Provider => {}
            Step::Name => self.step = Step::Provider,
            Step::Credential => self.step = Step::Name,
            Step::Params => {
                if self.field_idx > 0 {
                    self.field_idx -= 1;
                } else {
                    self.step = Step::Credential;
                }
            }
            Step::Interval => {
                let visible = self.visible_fields().len();
                self.field_idx = visible.saturating_sub(1);
                self.step = Step::Params;
            }
            Step::Summary => self.step = Step::Interval,
        }
    }

    fn cycle_choice(&mut self, delta: isize) {
        if self.step != Step::Params {
            return;
        }
        let visible = self.visible_fields();
        let Some(&idx) = visible.get(self.field_idx) else {
            return;
        };
        if let FieldKind::Choice {
            options, selected, ..
        } = &mut self.fields[idx].kind
        {
            let len = options.len() as isize;
            *selected = ((*selected as isize + delta).rem_euclid(len)) as usize;
        }
    }

    /// Modo OAuth só existe para provedores com login integrado.
    fn oauth_active(&self) -> bool {
        self.oauth_mode && supports_login(self.provider)
    }

    /// Login nativo: fluxo do cliente do próprio provedor (9router), sem app registrado.
    fn native_login(&self) -> bool {
        self.oauth_active() && !requires_client_registration(self.provider)
    }

    /// Linhas do formulário no passo Credencial em modo OAuth: provedores de
    /// registro usam client_id + tenant/secret + confirmação; nativos usam as
    /// opções do fluxo (0..n) + linha de confirmação.
    fn oauth_rows(&self) -> usize {
        if requires_client_registration(self.provider) {
            3 // client_id, tenant (Azure) ou secret_env (demais), iniciar/continuar
        } else {
            self.native_options.len() + 1
        }
    }

    fn oauth_text_mut(&mut self) -> Option<&mut String> {
        if !requires_client_registration(self.provider) {
            return self
                .native_options
                .get_mut(self.oauth_idx)
                .and_then(|f| match &mut f.kind {
                    FieldKind::Text(value) => Some(value),
                    FieldKind::Choice { .. } => None,
                });
        }
        match (self.provider, self.oauth_idx) {
            (Provider::AzureOpenAi, 1) => Some(&mut self.oauth_tenant),
            (_, 0) => Some(&mut self.oauth_client_id),
            (_, 1) => Some(&mut self.oauth_secret_env),
            _ => None,
        }
    }

    /// Alterna a opção Choice da linha nativa atual; mudar opção de autenticação
    /// invalida o login/metadata anterior (identidade mudou).
    fn cycle_native_option(&mut self, delta: isize) {
        if !self.native_login() {
            return;
        }
        if let Some(field) = self.native_options.get_mut(self.oauth_idx) {
            if let FieldKind::Choice {
                options, selected, ..
            } = &mut field.kind
            {
                let len = options.len() as isize;
                *selected = ((*selected as isize + delta).rem_euclid(len)) as usize;
                self.invalidate_oauth();
            }
        }
    }

    /// Qualquer mudança de identidade (modo, client_id, tenant, secret_env ou
    /// opção nativa) invalida o login anterior: o próximo login ganha novo
    /// credential_id e o Save exige nova autorização (ou a config existente intacta).
    fn invalidate_oauth(&mut self) {
        self.oauth_existing = None;
        self.oauth_authorized = None;
        self.oauth_pending_id = None;
        self.oauth_login_params.clear();
    }

    /// Config OAuth atual do formulário; reutiliza o credential_id pendente para
    /// que novas tentativas do MESMO login mantenham o mesmo identificador.
    fn current_oauth_config(&mut self) -> OAuthConfig {
        let mut config = OAuthConfig::new(self.oauth_client_id.trim().to_string());
        if requires_client_registration(self.provider) {
            if self.provider == Provider::AzureOpenAi {
                let tenant = self.oauth_tenant.trim();
                if !tenant.is_empty() {
                    config.tenant = Some(tenant.to_string());
                }
            } else {
                let secret = self.oauth_secret_env.trim();
                if !secret.is_empty() {
                    config.client_secret_env = Some(secret.to_string());
                }
            }
        } else {
            // Opções não secretas do fluxo nativo (region/auth_method/start_url);
            // tokens e segredos NUNCA entram aqui.
            for field in &self.native_options {
                match &field.kind {
                    FieldKind::Text(value) => {
                        let value = value.trim();
                        if !value.is_empty() {
                            config.options.insert(field.key.into(), value.to_string());
                        }
                    }
                    FieldKind::Choice {
                        options, selected, ..
                    } => {
                        config
                            .options
                            .insert(field.key.into(), options[*selected].0.into());
                    }
                }
            }
        }
        if let Some(id) = &self.oauth_pending_id {
            config.credential_id = id.clone();
        }
        config
    }

    /// Enter no passo Credencial em modo OAuth: percorre os campos e, na linha de
    /// confirmação, inicia o login (ou avança quando já há autorização).
    fn enter_oauth(&mut self) -> Action {
        if self.oauth_idx + 1 < self.oauth_rows() {
            if let Err(message) = self.validate_oauth_field() {
                self.error = Some(message);
                return Action::Continue;
            }
            self.error = None;
            self.oauth_idx += 1;
            return Action::Continue;
        }
        if self.oauth_authorized.is_some() || self.oauth_existing.is_some() {
            self.error = None;
            self.advance();
            return Action::Continue;
        }
        self.start_oauth_login()
    }

    fn validate_oauth_field(&self) -> Result<(), String> {
        if !requires_client_registration(self.provider) {
            // Linhas nativas: opções de texto (region/start_url) sem espaços/controle.
            if let Some(field) = self.native_options.get(self.oauth_idx) {
                if let FieldKind::Text(value) = &field.kind {
                    let value = value.trim();
                    if field.key == "database_path" {
                        if value.chars().any(char::is_control) {
                            return Err("Caminho inválido.".into());
                        }
                        return Ok(());
                    }
                    if !value.is_empty()
                        && value.chars().any(|c| c.is_control() || c.is_whitespace())
                    {
                        return Err(format!(
                            "{} inválido(a): sem espaços ou caracteres de controle.",
                            field.label
                        ));
                    }
                }
            }
            return Ok(());
        }
        match (self.provider, self.oauth_idx) {
            (_, 0) => {
                if self.oauth_client_id.trim().is_empty() {
                    return Err("Informe o client_id do SEU aplicativo OAuth registrado.".into());
                }
                Ok(())
            }
            (Provider::AzureOpenAi, 1) => {
                let tenant = self.oauth_tenant.trim();
                if !tenant.is_empty()
                    && !tenant
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))
                {
                    return Err("Tenant inválido (letras, números, '-' e '.'); vazio usa \"organizations\".".into());
                }
                Ok(())
            }
            (_, 1) => {
                let secret = self.oauth_secret_env.trim();
                if !secret.is_empty() && !valid_env_name(secret) {
                    return Err(
                        "Informe apenas o NOME da variável do client secret, nunca o valor.".into(),
                    );
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Emite a ação de login para o chamador executar o fluxo (navegador/device).
    fn start_oauth_login(&mut self) -> Action {
        let config = self.current_oauth_config();
        // Fixa o credential_id gerado para manter o mesmo ID entre repetições.
        self.oauth_pending_id = Some(config.credential_id.clone());
        match config.validate(self.provider) {
            Ok(()) => {
                self.error = None;
                Action::Login {
                    provider: self.provider,
                    config,
                }
            }
            Err(error) => {
                self.error = Some(error.to_string());
                Action::Continue
            }
        }
    }

    /// Login OAuth concluído com sucesso: registra a config autorizada, aplica os
    /// params públicos devolvidos (metadados seguros projetados pelo núcleo, nunca
    /// tokens) aos campos do rascunho e avança. Campos ocultos no modo nativo são
    /// gravados a partir de `oauth_login_params` no Save.
    pub(crate) fn login_finished(&mut self, config: OAuthConfig, params: BTreeMap<String, String>) {
        self.oauth_pending_id = Some(config.credential_id.clone());
        self.oauth_authorized = Some(config);
        for (key, value) in &params {
            if let Some(field) = self.fields.iter_mut().find(|f| f.key == key.as_str()) {
                match &mut field.kind {
                    FieldKind::Text(text) => *text = value.clone(),
                    FieldKind::Choice {
                        options, selected, ..
                    } => {
                        if let Some(pos) = options.iter().position(|(v, _)| *v == value) {
                            *selected = pos;
                        }
                    }
                }
            }
        }
        self.oauth_login_params = params;
        self.error = None;
        let notice = if self.provider == Provider::Cursor {
            "Sessão Cursor importada em memória. Revise e grave para persistir no Keychain."
                .to_string()
        } else if self.provider == Provider::XiaomiMimo {
            "Chave MiMo autorizada. Para consultar saldo/plano, configure platform_session_env com a sessão web; não repita o código.".to_string()
        } else {
            "Login autorizado; parâmetros da conta preenchidos automaticamente.".to_string()
        };
        self.notice = Some(notice);
        if self.step == Step::Credential {
            self.advance();
        }
    }

    /// `aws sso login` concluído: o perfil já estava configurado; segue o fluxo.
    pub(crate) fn aws_login_finished(&mut self) {
        self.error = None;
        self.notice = Some("Login SSO do AWS CLI concluído.".into());
        if self.step == Step::Credential {
            self.advance();
        }
    }

    /// `bl auth login --console` concluído no perfil dedicado. O Cuotai
    /// não armazena nada; basta mostrar o sucesso transitório e avançar.
    pub(crate) fn bailian_login_finished(&mut self) {
        self.error = None;
        self.notice = Some("Login `bl auth login --console` concluído no perfil dedicado.".into());
        if self.step == Step::Credential {
            self.advance();
        }
    }

    pub(crate) fn handle(&mut self, key: KeyEvent) -> Action {
        if key.kind != KeyEventKind::Press {
            return Action::Continue;
        }
        if key.code == KeyCode::Esc {
            return Action::Cancel;
        }
        // Aviso de sucesso é transitório: some na primeira tecla seguinte.
        self.notice = None;
        // Formulário OAuth do passo Credencial (apenas provedores com login integrado).
        if self.step == Step::Credential && self.oauth_active() {
            match key.code {
                KeyCode::Enter => return self.enter_oauth(),
                KeyCode::Tab => {
                    // Tab volta para o modo variável de ambiente e invalida o login.
                    self.oauth_mode = false;
                    self.oauth_idx = 0;
                    self.invalidate_oauth();
                    self.error = None;
                    return Action::Continue;
                }
                KeyCode::F(2) => {
                    // (Re)login explícito: novo credential_id; o login existente
                    // permanece utilizável se a nova tentativa falhar/cancelar.
                    self.oauth_authorized = None;
                    self.oauth_pending_id = None;
                    self.oauth_login_params.clear();
                    return self.start_oauth_login();
                }
                KeyCode::Up => {
                    self.oauth_idx = self.oauth_idx.saturating_sub(1);
                    return Action::Continue;
                }
                KeyCode::Down => {
                    self.oauth_idx = (self.oauth_idx + 1).min(self.oauth_rows() - 1);
                    return Action::Continue;
                }
                KeyCode::Left => {
                    self.cycle_native_option(-1);
                    return Action::Continue;
                }
                KeyCode::Right => {
                    self.cycle_native_option(1);
                    return Action::Continue;
                }
                KeyCode::BackTab => {
                    self.error = None;
                    if self.oauth_idx > 0 {
                        self.oauth_idx -= 1;
                    } else {
                        self.back();
                    }
                    return Action::Continue;
                }
                _ => {}
            }
        }
        match (key.code, key.modifiers) {
            (KeyCode::Enter, _) => {
                if self.step == Step::Summary {
                    return match self.build_config() {
                        Ok(config) => Action::Save(config),
                        Err(message) => {
                            self.error = Some(message);
                            Action::Continue
                        }
                    };
                }
                match self.validate_step() {
                    Ok(()) => {
                        self.error = None;
                        self.advance();
                    }
                    Err(message) => self.error = Some(message),
                }
            }
            (KeyCode::BackTab, _) => {
                self.error = None;
                self.back();
            }
            (KeyCode::Tab, _)
                if self.step == Step::Credential
                    && self.provider != Provider::AwsBedrock
                    && supports_login(self.provider) =>
            {
                self.oauth_mode = true;
                self.oauth_idx = 0;
                self.invalidate_oauth();
                self.error = None;
            }
            (KeyCode::F(2), _)
                if self.step == Step::Credential && self.provider == Provider::AwsBedrock =>
            {
                let profile = self.credential.trim().to_string();
                if profile.is_empty() {
                    self.error =
                        Some("Informe o perfil AWS SSO já configurado antes do login.".into());
                } else {
                    self.error = None;
                    return Action::AwsLogin { profile };
                }
            }
            (KeyCode::F(2), _)
                if self.step == Step::Credential && self.provider == Provider::AlibabaTokenPlan =>
            {
                // Login `bl` grava no próprio cofre; quota usa este perfil
                // quando não houver Cookie explícito em `params.session_env`.
                // Isolar por conta evita consultar o perfil default de outra conta.
                let mut params = match self
                    .editing
                    .and_then(|i| self.base.accounts.get(i))
                    .filter(|a| a.provider == self.provider)
                {
                    Some(a) => a.params.clone(),
                    None => BTreeMap::new(),
                };
                let profile = ensure_cli_profile(&mut params);
                self.credential_params = Some(params);
                self.error = None;
                return Action::BailianConsoleLogin {
                    console_site: "international".into(),
                    profile,
                };
            }
            (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => {
                if let Some(buf) = self.current_text_mut() {
                    buf.clear();
                    self.after_credential_edit();
                }
            }
            (KeyCode::Backspace, _) => {
                if let Some(buf) = self.current_text_mut() {
                    buf.pop();
                    self.after_credential_edit();
                }
            }
            (KeyCode::Up, _) => match self.step {
                Step::Provider => {
                    self.provider_sel = self.provider_sel.saturating_sub(1);
                }
                Step::Params => {
                    if self.current_is_choice() {
                        self.cycle_choice(-1);
                    } else {
                        self.field_idx = self.field_idx.saturating_sub(1);
                    }
                }
                Step::Summary => {
                    self.summary_scroll = self.summary_scroll.saturating_sub(1);
                }
                _ => {}
            },
            (KeyCode::Down, _) => match self.step {
                Step::Provider => {
                    self.provider_sel = (self.provider_sel + 1).min(Provider::ALL.len() - 1);
                }
                Step::Params => {
                    if self.current_is_choice() {
                        self.cycle_choice(1);
                    } else {
                        let max = self.visible_fields().len().saturating_sub(1);
                        self.field_idx = (self.field_idx + 1).min(max);
                    }
                }
                Step::Summary => {
                    self.summary_scroll = self.summary_scroll.saturating_add(1);
                }
                _ => {}
            },
            (KeyCode::Left, _) => {
                if self.step == Step::Params {
                    self.cycle_choice(-1);
                }
            }
            (KeyCode::Right, _) => {
                if self.step == Step::Params {
                    self.cycle_choice(1);
                }
            }
            (KeyCode::Char(c), m) if m.is_empty() || m == KeyModifiers::SHIFT => {
                if self.step == Step::Interval {
                    if c.is_ascii_digit() && self.interval.len() < 5 {
                        if let Some(buf) = self.current_text_mut() {
                            buf.push(c);
                        }
                    }
                } else if matches!(self.step, Step::Name | Step::Credential | Step::Params)
                    && !c.is_control()
                {
                    // Aceita texto colado/digitado, inclusive caminhos longos (Azure).
                    if let Some(buf) = self.current_text_mut() {
                        buf.push(c);
                    }
                }
                self.after_credential_edit();
            }
            _ => {}
        }
        Action::Continue
    }

    /// Botões clicáveis do rodapé: (retângulo exato de 1 linha, tecla
    /// equivalente, rótulo). `inner` é o interior com borda de `chunks[3]`.
    /// Usado tanto por `draw` (renderiza) quanto por `handle_mouse` (hit-test),
    /// garantindo que a geometria nunca divirja. `Voltar` é omitido no passo
    /// Provedor (não há para onde voltar); `Próximo` vira `Gravar` na Revisão.
    /// As teclas reproduzem os atalhos já documentados na dica do rodapé.
    fn footer_buttons(&self, inner: Rect) -> Vec<(Rect, KeyCode, &'static str)> {
        let mut entries: Vec<(&'static str, KeyCode)> = Vec::new();
        if self.step != Step::Provider {
            entries.push(("Voltar", KeyCode::BackTab));
        }
        entries.push((
            if self.step == Step::Summary {
                "Gravar"
            } else {
                "Próximo"
            },
            KeyCode::Enter,
        ));
        entries.push(("Cancelar", KeyCode::Esc));
        let mut buttons = Vec::new();
        let mut x = inner.x;
        for (label, code) in entries {
            let width = label.len() as u16 + 2; // colchetes
            if x.saturating_add(width) > inner.x.saturating_add(inner.width) {
                break;
            }
            buttons.push((
                Rect {
                    x,
                    y: inner.y,
                    width,
                    height: 1,
                },
                code,
                label,
            ));
            x = x.saturating_add(width + 2); // gap entre botões
        }
        buttons
    }

    /// Clique/rolagem do mouse. `area` é o retângulo completo em que `draw` foi
    /// renderizado (o `frame.area()` do chamador); as coordenadas do evento são
    /// absolutas de tela. A geometria de hit-test replica exatamente o layout de
    /// `draw`/`draw_*` (mesmos offsets e linhas de ajuda condicionais).
    ///
    /// Rolagem equivale a ↑/↓ (nunca Enter): rola a lista de provedores, as
    /// opções/campos e a revisão — nunca grava, apaga ou inicia login OAuth.
    /// Clique esquerdo foca a linha sob o cursor; clicar na linha JÁ focada
    /// ativa o controle óbvio dela (opção Choice avança; a linha de confirmação
    /// OAuth equivale a Enter via `handle`). Nenhum clique equivale a Enter
    /// genericamente e cliques fora do corpo são ignorados.
    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent, area: Rect) -> Action {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                return self.handle(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
            }
            MouseEventKind::ScrollDown => {
                return self.handle(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            }
            MouseEventKind::Down(MouseButton::Left) => {}
            _ => return Action::Continue,
        }
        // Mesma guarda de tamanho mínimo e mesmo split vertical de `draw`.
        if area.width < 60 || area.height < 20 {
            return Action::Continue;
        }
        let chunks = wizard_chunks(area);
        let (x, y) = (mouse.column, mouse.row);
        // Botões do rodapé: retângulos exatos renderizados por `draw`; cada um
        // equivale à tecla correspondente via `handle` (nunca clique=Enter
        // genérico — [Gravar] só existe no próprio botão do passo Revisão).
        let footer_inner = Block::default().borders(Borders::ALL).inner(chunks[3]);
        for (rect, code, _) in self.footer_buttons(footer_inner) {
            if y == rect.y && x >= rect.x && x < rect.x.saturating_add(rect.width) {
                return self.handle(KeyEvent::new(code, KeyModifiers::NONE));
            }
        }
        let inner = Block::default().borders(Borders::ALL).inner(chunks[1]);
        if x < inner.x
            || x >= inner.x.saturating_add(inner.width)
            || y < inner.y
            || y >= inner.y.saturating_add(inner.height)
        {
            return Action::Continue;
        }
        let row = y - inner.y;
        let enter = || KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let right = || KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        match self.step {
            // draw_provider: linha 0 é o cabeçalho; provedores começam na linha 1
            // a partir de `provider_offset`, no máximo `visible` (mínimo 1).
            Step::Provider => {
                if row == 0 {
                    return Action::Continue;
                }
                let visible = inner.height.saturating_sub(2).max(1) as usize;
                let idx = self.provider_offset as usize + (row as usize - 1);
                if idx
                    < Provider::ALL
                        .len()
                        .min(self.provider_offset as usize + visible)
                {
                    self.provider_sel = idx;
                }
            }
            // draw_params: cada campo ocupa 1 linha + 1 linha de ajuda quando focado.
            Step::Params => {
                let visible = self.visible_fields();
                let mut line = 0u16;
                for (pos, &i) in visible.iter().enumerate() {
                    let block = 1 + u16::from(pos == self.field_idx);
                    if row >= line && row < line + block {
                        if pos != self.field_idx {
                            self.field_idx = pos;
                        } else if row == line
                            && matches!(self.fields[i].kind, FieldKind::Choice { .. })
                        {
                            return self.handle(right());
                        }
                        return Action::Continue;
                    }
                    line += block;
                }
            }
            // draw_credential em modo OAuth: linhas clicáveis são as linhas de
            // formulário e a linha de confirmação (focar; segundo clique ativa).
            Step::Credential if self.oauth_active() => {
                if self.provider == Provider::Cursor {
                    // 4 linhas descritivas + 1 em branco; opcional linha de path
                    // (+ sua ajuda); 1 em branco; linha de ação.
                    let mut line = 5u16;
                    let has_path = matches!(
                        self.native_options.first().map(|f| &f.kind),
                        Some(FieldKind::Text(_))
                    );
                    if has_path {
                        if row == line {
                            self.oauth_idx = 0;
                            return Action::Continue;
                        }
                        line += 2; // path + ajuda fixa
                    }
                    line += 1; // em branco
                    if row == line {
                        let confirm = self.native_options.len();
                        if self.oauth_idx == confirm {
                            return self.handle(enter());
                        }
                        self.oauth_idx = confirm;
                    }
                    return Action::Continue;
                }
                if !requires_client_registration(self.provider) {
                    // header + em branco + aviso experimental + avisos por
                    // provedor + consentimento + em branco; depois as opções.
                    let mut line = 3u16;
                    line += u16::from(self.provider == Provider::Qwen);
                    line += u16::from(self.provider == Provider::Claude);
                    line += u16::from(self.provider == Provider::XiaomiMimo);
                    line += 2;
                    for (i, field) in self.native_options.iter().enumerate() {
                        let block = 1 + u16::from(i == self.oauth_idx);
                        if row >= line && row < line + block {
                            if i != self.oauth_idx {
                                self.oauth_idx = i;
                            } else if row == line && matches!(field.kind, FieldKind::Choice { .. })
                            {
                                return self.handle(right());
                            }
                            return Action::Continue;
                        }
                        line += block;
                    }
                    if row == line {
                        let confirm = self.native_options.len();
                        if self.oauth_idx == confirm {
                            return self.handle(enter());
                        }
                        self.oauth_idx = confirm;
                    }
                    return Action::Continue;
                }
                // Registro: header + em branco; linhas client_id e tenant/secret
                // (+ ajuda sob a focada); linha de confirmação.
                let mut line = 2u16;
                for i in 0..2 {
                    let block = 1 + u16::from(i == self.oauth_idx);
                    if row >= line && row < line + block {
                        if self.oauth_idx != i {
                            self.oauth_idx = i;
                        }
                        return Action::Continue;
                    }
                    line += block;
                }
                if row == line {
                    if self.oauth_idx == 2 {
                        return self.handle(enter());
                    }
                    self.oauth_idx = 2;
                }
            }
            // Name/Interval/Credential em modo variável de ambiente: um único
            // campo de texto já focado; Summary: só rolagem (tratada acima).
            _ => {}
        }
        Action::Continue
    }

    /// Edição em campo de texto OAuth (não na linha de confirmação) invalida o
    /// login/metadata anterior: identidade mudou, próximo login usa novo ID.
    fn after_credential_edit(&mut self) {
        if self.step == Step::Credential
            && self.oauth_active()
            && self.oauth_idx + 1 < self.oauth_rows()
        {
            self.invalidate_oauth();
        }
    }

    fn current_is_choice(&self) -> bool {
        let visible = self.visible_fields();
        visible
            .get(self.field_idx)
            .map(|&i| matches!(self.fields[i].kind, FieldKind::Choice { .. }))
            .unwrap_or(false)
    }

    /// Validação por passo no Enter: o erro aparece onde a correção é feita.
    fn validate_step(&self) -> Result<(), String> {
        match self.step {
            Step::Name => {
                if self.name.trim().is_empty() {
                    return Err("Informe um nome para a conta.".into());
                }
                if self.name_taken() {
                    return Err("Já existe uma conta com esse nome.".into());
                }
                Ok(())
            }
            Step::Credential => {
                let credential = self.credential.trim();
                if self.provider == Provider::AwsBedrock {
                    if credential.is_empty() {
                        return Err("Informe o perfil AWS (AWS CLI v2).".into());
                    }
                    return Ok(());
                }
                if self.provider == Provider::AlibabaTokenPlan {
                    return Ok(());
                }
                if credential.is_empty() {
                    return Err("Informe o NOME da variável de ambiente com a credencial.".into());
                }
                // Nunca ecoa o conteúdo digitado: pode ser um token colado por engano.
                if !valid_env_name(credential) {
                    return Err("Nome de variável inválido ([A-Za-z_][A-Za-z0-9_]*). Não cole o token: informe apenas o nome da variável exportada.".into());
                }
                Ok(())
            }
            Step::Params => {
                let visible = self.visible_fields();
                let Some(&i) = visible.get(self.field_idx) else {
                    return Ok(());
                };
                let field = &self.fields[i];
                if let FieldKind::Text(value) = &field.kind {
                    let value = value.trim();
                    if value.is_empty() {
                        if field.required {
                            return Err(format!("{} é obrigatório.", field.label));
                        }
                        return Ok(()); // opcional em branco é removido só ao salvar
                    }
                    let mut known = BTreeMap::new();
                    for &j in &visible {
                        if let FieldKind::Text(v) = &self.fields[j].kind {
                            known.insert(self.fields[j].key.to_string(), v.trim().to_string());
                        }
                    }
                    return validate_param(field.key, value, &known);
                }
                Ok(())
            }
            Step::Interval => self.parse_interval().map(|_| ()),
            _ => Ok(()),
        }
    }

    fn name_taken(&self) -> bool {
        let name = self.name.trim();
        self.base
            .accounts
            .iter()
            .enumerate()
            .any(|(i, a)| Some(i) != self.editing && a.name.eq_ignore_ascii_case(name))
    }

    fn parse_interval(&self) -> Result<Option<u64>, String> {
        // Campo vazio: herda o intervalo global (sem override individual).
        if self.interval.trim().is_empty() {
            return Ok(None);
        }
        let interval: u64 = self
            .interval
            .trim()
            .parse()
            .map_err(|_| "Intervalo inválido.".to_string())?;
        if !(MIN_INTERVAL..=MAX_INTERVAL).contains(&interval) {
            return Err(format!(
                "Intervalo deve ficar entre {MIN_INTERVAL} e {MAX_INTERVAL} segundos."
            ));
        }
        Ok(Some(interval))
    }

    fn env_status(&self) -> (&'static str, Color) {
        if !valid_env_name(self.credential.trim()) {
            return ("NOME INVÁLIDO", DRACULA_RED);
        }
        // Mesma regra do backend: valor presente e não vazio após trim.
        match std::env::var(self.credential.trim()) {
            Ok(value) if !value.trim().is_empty() => ("PRESENTE", DRACULA_GREEN),
            _ => ("AUSENTE", DRACULA_YELLOW),
        }
    }

    fn build_config(&self) -> Result<Config, String> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err("Informe um nome para a conta.".into());
        }
        if self.name_taken() {
            return Err(format!(
                "Já existe uma conta chamada \"{}\".",
                crate::ui::safe(&name)
            ));
        }

        let credential = self.credential.trim().to_string();
        // Mesmo provedor: conserva chaves de params não gerenciadas pelo assistente;
        // provedor trocado: começa vazio (incompatíveis já foram descartados na troca).
        // Quando o F2 do Bailian preencheu `credential_params` com `cli_profile`,
        // essa versão enriquecida prevalece para preservar o profile dedicado.
        let mut params: BTreeMap<String, String> =
            self.credential_params.clone().unwrap_or_else(|| {
                match self.editing.and_then(|i| self.base.accounts.get(i)) {
                    Some(a) if a.provider == self.provider => a.params.clone(),
                    _ => BTreeMap::new(),
                }
            });
        for key in managed_keys(self.provider) {
            params.remove(*key);
        }
        // Modo OAuth: credencial vive no armazenamento seguro (nunca no TOML) e o
        // Save só é permitido com login autorizado nesta sessão OU a config OAuth
        // existente preservada (sem mudança de identidade/modo).
        let oauth = if self.oauth_active() {
            match self
                .oauth_authorized
                .clone()
                .or_else(|| self.oauth_existing.clone())
            {
                Some(config) => Some(config),
                None => {
                    return Err(
                        "Faça o login OAuth (Enter em \"Iniciar login\") antes de gravar, ou volte à variável de ambiente com Tab."
                            .into(),
                    )
                }
            }
        } else {
            None
        };
        let credential_env = if self.provider == Provider::AwsBedrock {
            if credential.is_empty() {
                return Err("Informe o perfil AWS (AWS CLI v2).".into());
            }
            params.insert("profile".into(), credential);
            None
        } else if oauth.is_some() || self.provider == Provider::AlibabaTokenPlan {
            None
        } else {
            if credential.is_empty() {
                return Err("Informe o NOME da variável de ambiente com a credencial.".into());
            }
            // Nunca ecoa o conteúdo digitado: pode ser um token colado por engano.
            if !valid_env_name(&credential) {
                return Err("Nome de variável inválido ([A-Za-z_][A-Za-z0-9_]*). Não cole o token: informe apenas o nome da variável exportada.".into());
            }
            Some(credential)
        };

        let visible = self.visible_fields();
        for &i in &visible {
            let field = &self.fields[i];
            match &field.kind {
                FieldKind::Text(value) => {
                    let value = value.trim().to_string();
                    if value.is_empty() {
                        if field.required {
                            return Err(format!("{} é obrigatório.", field.label));
                        }
                        continue;
                    }
                    validate_param(field.key, &value, &params)?;
                    params.insert(field.key.into(), value);
                }
                FieldKind::Choice {
                    options,
                    selected,
                    omit_default,
                } => {
                    if !(*omit_default && *selected == 0) {
                        params.insert(field.key.into(), options[*selected].0.into());
                    }
                }
            }
        }

        // Login nativo: params controlados pela identidade (ocultos no passo
        // Parâmetros) vêm dos metadados públicos do login desta sessão; ao editar
        // conta já autenticada sem novo login, preserva os valores existentes.
        if oauth.is_some() && !requires_client_registration(self.provider) {
            if self.oauth_authorized.is_some() {
                params.extend(self.oauth_login_params.clone());
            } else if let Some(account) = self
                .editing
                .and_then(|i| self.base.accounts.get(i))
                .filter(|a| a.provider == self.provider)
            {
                for key in native_param_keys(self.provider) {
                    if let Some(value) = account
                        .params
                        .get(*key)
                        .filter(|_| *key != "profile_arn" || self.provider != Provider::Kiro)
                    {
                        params.insert((*key).into(), value.clone());
                    }
                }
            }
        }

        let interval_override = self.parse_interval()?;

        let account = Account {
            name,
            provider: self.provider,
            credential_env,
            oauth,
            params,
            refresh_seconds: interval_override,
        };
        // O intervalo global nunca é tocado aqui: o override é individual e o
        // global permanece como fallback para contas sem override.
        let mut config = self.base.clone();
        match self.editing {
            Some(i) if i < config.accounts.len() => config.accounts[i] = account,
            _ => config.accounts.push(account),
        }
        Ok(config)
    }

    fn summary_lines(&self, path: &Path) -> Vec<Line<'static>> {
        let dim = Style::default().fg(DRACULA_COMMENT);
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Nome: ", dim),
                Span::raw(crate::ui::safe(self.name.trim()).into_owned()),
            ]),
            Line::from(vec![
                Span::styled("Provedor: ", dim),
                Span::raw(self.provider.label()),
            ]),
        ];
        if self.provider == Provider::AwsBedrock {
            lines.push(Line::from(vec![
                Span::styled("Perfil AWS: ", dim),
                Span::raw(crate::ui::safe(self.credential.trim()).into_owned()),
            ]));
        } else if self.provider == Provider::AlibabaTokenPlan {
            // Quota Token Plan consulta, por padrão, o perfil dedicado do `bl`.
            // session_env (Cookie) é override opcional; refletir isso no sumário.
            if let Some(env) = self
                .credential_params
                .as_ref()
                .and_then(|p| p.get("session_env"))
                .or_else(|| {
                    self.editing
                        .and_then(|i| self.base.accounts.get(i))
                        .and_then(|a| a.params.get("session_env"))
                })
            {
                if !env.trim().is_empty() {
                    lines.push(Line::from(format!(
                        "Quota: Cookie Qwen Cloud (params.session_env = {})",
                        crate::ui::safe(env.trim())
                    )));
                    lines.push(Line::from(
                        "Perfil `bl` dedicado continua gravado em params.cli_profile.",
                    ));
                } else {
                    lines.push(Line::from(
                        "Quota: perfil dedicado do `bl` (params.cli_profile); session_env vazio.",
                    ));
                }
            } else if self
                .credential_params
                .as_ref()
                .and_then(|p| p.get("cli_profile"))
                .is_some()
                || self
                    .editing
                    .and_then(|i| self.base.accounts.get(i))
                    .and_then(|a| a.params.get("cli_profile"))
                    .is_some()
            {
                lines.push(Line::from(
                    "Quota: perfil dedicado do `bl` (params.cli_profile); session_env vazio = rota CLI.",
                ));
            } else {
                lines.push(Line::from(
                    "Quota: perfil dedicado do `bl` ainda não configurado (use F2 na etapa Credencial).",
                ));
            }
        } else if self.oauth_active() {
            let (status, color) = if self.oauth_authorized.is_some() {
                ("LOGIN AUTORIZADO", DRACULA_GREEN)
            } else if self.oauth_existing.is_some() {
                ("LOGIN EXISTENTE", DRACULA_GREEN)
            } else {
                ("LOGIN PENDENTE", DRACULA_YELLOW)
            };
            let mode_label = if self.provider == Provider::Cursor {
                "Sessão pessoal Cursor importada  "
            } else if requires_client_registration(self.provider) {
                "OAuth (app do usuário)  "
            } else {
                "Login nativo (experimental)  "
            };
            lines.push(Line::from(vec![
                Span::styled("Autenticação: ", dim),
                Span::raw(mode_label),
                Span::styled(status, Style::default().fg(color)),
            ]));
            if requires_client_registration(self.provider) {
                lines.push(Line::from(vec![
                    Span::styled("Client ID: ", dim),
                    Span::raw(crate::ui::safe(self.oauth_client_id.trim()).into_owned()),
                ]));
            } else {
                for field in &self.native_options {
                    let value = match &field.kind {
                        FieldKind::Text(v) => crate::ui::safe(v.trim()).into_owned(),
                        FieldKind::Choice {
                            options, selected, ..
                        } => options[*selected].0.to_string(),
                    };
                    lines.push(Line::from(format!("  {} = {value}", field.key)));
                }
            }
            lines.push(Line::from(Span::styled(
                "(o token fica no armazenamento seguro; nunca é exibido nem gravado no TOML)",
                dim,
            )));
        } else {
            let (status, color) = self.env_status();
            lines.push(Line::from(vec![
                Span::styled("Variável de credencial: ", dim),
                Span::raw(crate::ui::safe(self.credential.trim()).into_owned()),
                Span::raw("  "),
                Span::styled(status, Style::default().fg(color)),
            ]));
            lines.push(Line::from(Span::styled(
                "(o valor da credencial nunca é exibido nem gravado)",
                dim,
            )));
        }
        match self.build_config() {
            Ok(config) => {
                let account = config
                    .accounts
                    .get(self.editing.unwrap_or(config.accounts.len() - 1))
                    .expect("conta recém-construída");
                if account.params.is_empty() {
                    lines.push(Line::from(Span::styled("Parâmetros: nenhum", dim)));
                } else {
                    lines.push(Line::from(Span::styled("Parâmetros:", dim)));
                    for (key, value) in &account.params {
                        lines.push(Line::from(format!("  {key} = {}", crate::ui::safe(value))));
                    }
                }
                lines.push(Line::from(vec![
                    Span::styled("Intervalo desta conta: ", dim),
                    Span::raw(match account.refresh_seconds {
                        Some(seconds) => format!("{seconds}s (override individual)"),
                        None => format!("{}s (herdando global)", config.refresh_seconds),
                    }),
                ]));
                lines.push(Line::from(Span::styled(
                    format!("Intervalo global: {}s", config.refresh_seconds),
                    dim,
                )));
            }
            Err(message) => {
                lines.push(Line::from(Span::styled(
                    format!("Pendências: {message}"),
                    Style::default().fg(DRACULA_RED),
                )));
            }
        }
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled("Arquivo de destino: ", dim),
            Span::raw(path.display().to_string()),
        ]));
        if self.provider == Provider::AwsBedrock {
            lines.push(Line::from(Span::styled(
                "ATENÇÃO: AWS Cost Explorer cobra por consulta; prefira intervalos altos (ex.: 3600s).",
                Style::default().fg(DRACULA_YELLOW),
            )));
        }
        lines
    }

    pub(crate) fn draw(&mut self, frame: &mut Frame, path: &Path) {
        let area = frame.area();
        // Dracula frame background para o wizard inteiro; o restante do TUI
        // (dashboard) usa o mesmo padrão em outro ponto de desenho.
        frame.render_widget(
            Block::default().style(Style::default().bg(DRACULA_BG).fg(DRACULA_FG)),
            area,
        );
        if area.width < 60 || area.height < 20 {
            frame.render_widget(
                Paragraph::new(
                    "Assistente: amplie o terminal para pelo menos 60 × 20. Esc cancela.",
                )
                .wrap(Wrap { trim: true })
                .style(Style::default().bg(DRACULA_BG)),
                area,
            );
            return;
        }
        let chunks = wizard_chunks(area);

        let title = match self.step {
            Step::Provider => "Assistente — 1/6 Provedor",
            Step::Name => "Assistente — 2/6 Nome da conta",
            Step::Credential => "Assistente — 3/6 Credencial",
            Step::Params => "Assistente — 4/6 Parâmetros",
            Step::Interval => "Assistente — 5/6 Intervalo",
            Step::Summary => "Assistente — 6/6 Revisão",
        };
        frame.render_widget(
            Paragraph::new(title)
                .block(Block::default().borders(Borders::ALL))
                .style(Style::default().add_modifier(Modifier::BOLD)),
            chunks[0],
        );

        let body = Block::default().borders(Borders::ALL);
        let inner = body.inner(chunks[1]);
        frame.render_widget(body, chunks[1]);

        match self.step {
            Step::Provider => self.draw_provider(frame, inner),
            Step::Name => {
                let lines = vec![
                    Line::from("Nome único para identificar a conta no painel:"),
                    Line::default(),
                    Line::from(format!("> {}", crate::ui::safe(&self.name))),
                    Line::default(),
                    Line::from(Span::styled(
                        "Letras, números e espaços; não pode repetir outra conta.",
                        Style::default().fg(DRACULA_COMMENT),
                    )),
                ];
                frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
            }
            Step::Credential => self.draw_credential(frame, inner),
            Step::Params => self.draw_params(frame, inner),
            Step::Interval => self.draw_interval(frame, inner),
            Step::Summary => {
                let lines = self.summary_lines(path);
                let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
                let max_scroll = paragraph
                    .line_count(inner.width)
                    .saturating_sub(usize::from(inner.height))
                    .min(usize::from(u16::MAX)) as u16;
                self.summary_scroll = self.summary_scroll.min(max_scroll);
                frame.render_widget(paragraph.scroll((self.summary_scroll, 0)), inner);
            }
        }

        if let Some(error) = &self.error {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    error.clone(),
                    Style::default().fg(DRACULA_RED),
                ))
                .wrap(Wrap { trim: false }),
                chunks[2],
            );
        } else if let Some(notice) = &self.notice {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    notice.clone(),
                    Style::default().fg(DRACULA_GREEN),
                ))
                .wrap(Wrap { trim: false }),
                chunks[2],
            );
        }

        let hint = match self.step {
            Step::Provider => " ↑↓ navegar  Enter selecionar  Esc cancelar",
            Step::Params => {
                " Enter próximo  Shift+Tab voltar  ↑↓/←→ opções  Ctrl+U limpar  Esc cancelar"
            }
            Step::Summary => {
                " Enter GRAVAR no arquivo  ↑↓ rolar  Shift+Tab voltar  Esc cancelar sem gravar"
            }
            Step::Credential => {
                if self.provider == Provider::AwsBedrock {
                    " Enter próximo  F2 login SSO (perfil existente)  Shift+Tab voltar  Esc cancelar"
                } else if self.provider == Provider::AlibabaTokenPlan {
                    " Enter próximo  F2 bl auth login --console (internacional)  Shift+Tab voltar  Ctrl+U limpar  Esc cancela"
                } else if self.provider == Provider::Cursor {
                    " Enter próximo/confirmar • Tab alterna Admin API/importação • F2 reimporta • Esc cancela"
                } else if self.native_login() {
                    " Enter próximo/iniciar  F2 (re)login  ←→ opções  Tab variável de ambiente  ↑↓ campos  Shift+Tab voltar  Esc cancela"
                } else if self.oauth_active() {
                    " Enter próximo/iniciar  F2 (re)login  Tab variável de ambiente  ↑↓ campos  Shift+Tab voltar  Esc cancela"
                } else if supports_login(self.provider) {
                    " Enter próximo  Tab login no navegador  Shift+Tab voltar  Ctrl+U limpar  Esc cancelar"
                } else {
                    " Enter próximo  Shift+Tab voltar  Ctrl+U limpar  Esc cancelar"
                }
            }
            Step::Interval => {
                " Enter próximo  Shift+Tab voltar  Ctrl+U limpar  vazio herda global  Esc cancelar"
            }
            _ => " Enter próximo  Shift+Tab voltar  Ctrl+U limpar  Esc cancelar",
        };
        let footer = Block::default().borders(Borders::ALL);
        let footer_inner = footer.inner(chunks[3]);
        frame.render_widget(footer, chunks[3]);
        // Botões clicáveis: mesmos retângulos usados no hit-test do mouse.
        for (rect, _, label) in self.footer_buttons(footer_inner) {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!("[{label}]"),
                    Style::default()
                        .fg(DRACULA_CYAN)
                        .add_modifier(Modifier::BOLD),
                )),
                rect,
            );
        }
        let hint_area = Rect {
            x: footer_inner.x,
            y: footer_inner.y.saturating_add(1),
            width: footer_inner.width,
            height: footer_inner.height.saturating_sub(1),
        };
        frame.render_widget(
            Paragraph::new(hint)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(DRACULA_CYAN)),
            hint_area,
        );
    }

    fn draw_provider(&mut self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let visible = area.height.saturating_sub(2) as usize;
        let total = Provider::ALL.len();
        if self.provider_sel < self.provider_offset as usize {
            self.provider_offset = self.provider_sel as u16;
        }
        if self.provider_sel >= self.provider_offset as usize + visible {
            self.provider_offset = (self.provider_sel + 1 - visible) as u16;
        }
        let mut lines = vec![Line::from(Span::styled(
            "Setas navegam; Enter seleciona o provedor da conta.",
            Style::default().fg(DRACULA_COMMENT),
        ))];
        for (i, provider) in Provider::ALL
            .iter()
            .enumerate()
            .skip(self.provider_offset as usize)
            .take(visible.max(1))
        {
            let marker = if i == self.provider_sel { "▶ " } else { "  " };
            let style = if i == self.provider_sel {
                Style::default()
                    .fg(DRACULA_CYAN)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(
                format!("{marker}{}", provider.label()),
                style,
            )));
        }
        if self.provider_offset as usize + visible < total {
            lines.push(Line::from(Span::styled(
                "  …",
                Style::default().fg(DRACULA_COMMENT),
            )));
        }
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn draw_credential(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let mut lines: Vec<Line> = Vec::new();
        if self.provider == Provider::AwsBedrock {
            lines.push(Line::from(
                "Perfil do AWS CLI v2 usado para as credenciais:",
            ));
            lines.push(Line::default());
            lines.push(Line::from(format!(
                "> {}",
                crate::ui::safe(&self.credential)
            )));
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                "Configure antes com `aws configure` ou `aws configure sso`. ATENÇÃO: o Cost Explorer cobra por consulta.",
                Style::default().fg(DRACULA_COMMENT),
            )));
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                "F2 executa `aws sso login --use-device-code --no-browser` (somente perfis SSO já configurados).",
                Style::default().fg(DRACULA_COMMENT),
            )));
        } else if self.provider == Provider::AlibabaTokenPlan {
            lines.push(Line::from(
                "A quota Token Plan consulta, por padrão, o perfil dedicado do `bl` (`params.cli_profile`).",
            ));
            lines.push(Line::from(
                "`session_env` (Cookie de home.qwencloud.com) é override opcional para quem prefere a rota antiga.",
            ));
            lines.push(Line::from(
                "QWEN_API_KEY autentica apenas inferência; não é enviada à consulta.",
            ));
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                "F2 dispara `bl auth login --console --console-site international` no perfil dedicado. O Cuotai NÃO armazena nada; cancelar aqui NÃO desfaz o que o `bl` grava.",
                Style::default().fg(DRACULA_COMMENT),
            )));
        } else if self.provider == Provider::Cursor && self.oauth_active() {
            lines.push(Line::from(
                "Importar sessão pessoal do Cursor instalado neste computador",
            ));
            lines.push(Line::from(
                "Não inicia OAuth: faça login no aplicativo Cursor primeiro.",
            ));
            lines.push(Line::from("Somente após confirmar, serão lidos accessToken e machine ID do SQLite local (somente leitura)."));
            lines.push(Line::from(
                "Tokens não serão exibidos; a cópia só será salva no Keychain na revisão final.",
            ));
            lines.push(Line::default());
            if let Some(field) = self.native_options.first() {
                if let FieldKind::Text(path) = &field.kind {
                    lines.push(Line::from(format!(
                        "{} Banco Cursor (opcional): {}",
                        if self.oauth_idx == 0 { ">" } else { " " },
                        crate::ui::safe(path)
                    )));
                    lines.push(Line::from("Vazio usa instalação padrão. Insiders/perfil alternativo: informe caminho completo state.vscdb."));
                }
            }
            lines.push(Line::default());
            let action = if self.oauth_authorized.is_some() || self.oauth_existing.is_some() {
                "Sessão importada — Enter continua; F2 reimporta do Cursor"
            } else {
                "CONFIRMAR leitura e importação da sessão Cursor — Enter autoriza"
            };
            lines.push(Line::styled(action, Style::default().fg(DRACULA_YELLOW)));
        } else if self.oauth_active() && !requires_client_registration(self.provider) {
            // Login nativo: fluxo do cliente do provedor (referência 9router), sem app registrado.
            lines.push(Line::from(format!(
                "Login nativo com {} (Tab volta à variável de ambiente):",
                self.provider.label()
            )));
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                "Fluxo EXPERIMENTAL baseado no cliente nativo do provedor (interface interna, sem suporte oficial; pode falhar ou ser restringido pelo provedor a qualquer momento).",
                Style::default().fg(DRACULA_YELLOW),
            )));
            if self.provider == Provider::Qwen {
                lines.push(Line::styled("Qwen Code: OAuth gratuito descontinuado em 15/04/2026. Esta tentativa pode ser recusada pelo provedor; login não garante quota disponível.", Style::default().fg(DRACULA_YELLOW)));
            }
            if self.provider == Provider::Claude {
                lines.push(Line::from(Span::styled(
                    "Claude: o provedor pode restringir este fluxo a clientes próprios; se recusado, use a variável de ambiente.",
                    Style::default().fg(DRACULA_YELLOW),
                )));
            }
            if self.provider == Provider::XiaomiMimo {
                lines.push(Line::from(Span::styled(
                    "O código autoriza uma chave de inferência, não a sessão do console. Saldo e Token Plan usam platform_session_env; quota semanal do Desktop usa session_env.",
                    Style::default().fg(DRACULA_COMMENT),
                )));
            }
            lines.push(Line::from(Span::styled(
                "O consentimento acontece no navegador/código de dispositivo; nenhuma senha ou token é digitada neste terminal.",
                Style::default().fg(DRACULA_COMMENT),
            )));
            lines.push(Line::default());
            for (i, field) in self.native_options.iter().enumerate() {
                let current = self.oauth_idx == i;
                let marker = if current { "▶ " } else { "  " };
                let style = if current {
                    Style::default()
                        .fg(DRACULA_CYAN)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let value = match &field.kind {
                    FieldKind::Text(v) => crate::ui::safe(v).into_owned(),
                    FieldKind::Choice {
                        options, selected, ..
                    } => format!("< {} >", options[*selected].0),
                };
                lines.push(Line::from(Span::styled(
                    format!("{marker}{}: {value}", field.label),
                    style,
                )));
                if current {
                    lines.push(Line::from(Span::styled(
                        format!("    {}", field.help),
                        Style::default().fg(DRACULA_COMMENT),
                    )));
                }
            }
            let current = self.oauth_idx == self.native_options.len();
            let marker = if current { "▶ " } else { "  " };
            let style = if current {
                Style::default()
                    .fg(DRACULA_CYAN)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let confirm = if self.oauth_authorized.is_some() {
                "Login autorizado — Enter continua (F2 refaz com novo ID)".to_string()
            } else if self.oauth_existing.is_some() {
                "Login existente — Enter continua (F2 refaz o login, ex.: token expirado)"
                    .to_string()
            } else {
                format!(
                    "Entrar com {} no navegador — Enter inicia o login",
                    self.provider.label()
                )
            };
            lines.push(Line::from(Span::styled(
                format!("{marker}{confirm}"),
                style,
            )));
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                "Parâmetros da conta (IDs, região, produto) são preenchidos automaticamente após o login.",
                Style::default().fg(DRACULA_COMMENT),
            )));
        } else if self.oauth_active() {
            lines.push(Line::from(
                "Login OAuth com aplicativo registrado por VOCÊ (Tab volta à variável de ambiente):",
            ));
            lines.push(Line::default());
            let second: (&str, &str, &str) = if self.provider == Provider::AzureOpenAi {
                (
                    "Tenant (opcional)",
                    "Vazio usa \"organizations\" (contas em qualquer organização).",
                    self.oauth_tenant.as_str(),
                )
            } else {
                (
                    "Variável do client secret (opcional)",
                    "Apenas o NOME da variável exportada com o secret; nunca o valor.",
                    self.oauth_secret_env.as_str(),
                )
            };
            let rows = [
                (
                    "Client ID do app",
                    "Do SEU aplicativo registrado; o Cuotai não tem app próprio.",
                    self.oauth_client_id.as_str(),
                ),
                second,
            ];
            for (i, (label, help, value)) in rows.iter().enumerate() {
                let current = self.oauth_idx == i;
                let marker = if current { "▶ " } else { "  " };
                let style = if current {
                    Style::default()
                        .fg(DRACULA_CYAN)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                lines.push(Line::from(Span::styled(
                    format!("{marker}{label}: {}", crate::ui::safe(value)),
                    style,
                )));
                if current {
                    lines.push(Line::from(Span::styled(
                        format!("    {help}"),
                        Style::default().fg(DRACULA_COMMENT),
                    )));
                }
            }
            let current = self.oauth_idx == rows.len();
            let marker = if current { "▶ " } else { "  " };
            let style = if current {
                Style::default()
                    .fg(DRACULA_CYAN)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let confirm = if self.oauth_authorized.is_some() {
                "Login autorizado — Enter continua (F2 refaz com novo ID)"
            } else if self.oauth_existing.is_some() {
                "Login existente — Enter continua (F2 refaz o login, ex.: token expirado)"
            } else {
                "Iniciar login OAuth — Enter abre o fluxo (navegador/código de dispositivo)"
            };
            lines.push(Line::from(Span::styled(
                format!("{marker}{confirm}"),
                style,
            )));
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                oauth_registration_help(self.provider),
                Style::default().fg(DRACULA_COMMENT),
            )));
        } else {
            lines.push(Line::from(
                "NOME da variável de ambiente exportada com a credencial (nunca o valor):",
            ));
            lines.push(Line::default());
            lines.push(Line::from(format!(
                "> {}",
                crate::ui::safe(&self.credential)
            )));
            lines.push(Line::default());
            let (status, color) = self.env_status();
            let note = match status {
                "PRESENTE" => " (valor presente no ambiente; nunca exibido nem gravado)",
                "AUSENTE" => " (pode salvar mesmo assim e exportar antes de usar)",
                _ => "",
            };
            lines.push(Line::from(vec![
                Span::raw("Status no ambiente: "),
                Span::styled(status, Style::default().fg(color)),
                Span::styled(note, Style::default().fg(DRACULA_COMMENT)),
            ]));
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                credential_help(self.provider),
                Style::default().fg(DRACULA_COMMENT),
            )));
            lines.push(Line::default());
            if supports_login(self.provider) {
                let hint = if requires_client_registration(self.provider) {
                    "Tab alterna para login OAuth com aplicativo registrado por você."
                } else {
                    "Tab alterna para login nativo no navegador (experimental, sem registrar aplicativo)."
                };
                lines.push(Line::from(Span::styled(
                    hint,
                    Style::default().fg(DRACULA_COMMENT),
                )));
            } else if let Some(reason) = oauth_unsupported_reason(self.provider) {
                lines.push(Line::from(Span::styled(
                    format!("Login OAuth integrado indisponível: {reason}"),
                    Style::default().fg(DRACULA_COMMENT),
                )));
            }
        }
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
    }

    fn draw_params(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let visible = self.visible_fields();
        if visible.is_empty() {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from("Este provedor não tem parâmetros adicionais."),
                    Line::default(),
                    Line::from(Span::styled(
                        "Enter avança; Shift+Tab volta.",
                        Style::default().fg(DRACULA_COMMENT),
                    )),
                ]),
                area,
            );
            return;
        }
        let mut lines: Vec<Line> = Vec::new();
        for (pos, &i) in visible.iter().enumerate() {
            let field = &self.fields[i];
            let current = pos == self.field_idx;
            let marker = if current { "▶ " } else { "  " };
            let req = if field.required {
                " (obrigatório)"
            } else {
                ""
            };
            let style = if current {
                Style::default()
                    .fg(DRACULA_CYAN)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let value = match &field.kind {
                FieldKind::Text(v) => crate::ui::safe(v).into_owned(),
                FieldKind::Choice {
                    options, selected, ..
                } => format!("< {} >", options[*selected].0),
            };
            lines.push(Line::from(Span::styled(
                format!("{marker}{}{req}: {value}", field.label),
                style,
            )));
            if current {
                lines.push(Line::from(Span::styled(
                    format!("    {}", field.help),
                    Style::default().fg(DRACULA_COMMENT),
                )));
            }
        }
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
    }

    fn draw_interval(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        // Dracula prompt color (cyan) e amarelo suave para o alerta AWS.
        let prompt = Style::default().fg(DRACULA_CYAN);
        let accent = Style::default().fg(DRACULA_GREEN);
        let dim = Style::default().fg(DRACULA_COMMENT);
        let mut lines = vec![
            Line::from(Span::styled(
                format!(
                    "Intervalo individual desta conta em segundos ({MIN_INTERVAL}–{MAX_INTERVAL}); vazio herda o global:"
                ),
                prompt,
            )),
            Line::default(),
            Line::from(Span::styled(format!("> {}", self.interval), accent)),
        ];
        if self.provider == Provider::AwsBedrock {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                "ATENÇÃO: AWS Cost Explorer cobra por consulta (≈US$0,01). 300s pode gerar custo relevante; recomendado 3600s. O valor NÃO é alterado automaticamente.",
                Style::default().fg(DRACULA_YELLOW),
            )));
        }
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "Vale só para esta conta; vazio = global.",
            dim,
        )));
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
    }
}

fn valid_env_name(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Garante `params.cli_profile` com um nome estável e exclusivo para a conta.
/// O `bl` aceita `--config <name>` para isolar credenciais entre contas;
/// sem este perfil, duas contas distintas poderiam usar o `default` e
/// acabar vendo os dados uma da outra.
fn ensure_cli_profile(params: &mut BTreeMap<String, String>) -> String {
    const KEY: &str = "cli_profile";
    if let Some(existing) = params.get(KEY) {
        if !existing.is_empty() && is_valid_cli_profile(existing) {
            return existing.clone();
        }
    }
    // Sufixo aleatório curto para evitar colisão entre contas.
    let suffix: u32 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_nanos() as u32) % 1_000_000)
        .unwrap_or(0);
    let profile = format!("cuotai-{suffix:06}");
    params.insert(KEY.into(), profile.clone());
    profile
}

/// Nomes de perfil do `bl` aceitam letras, dígitos, hífen e underscore; até 32 chars.
fn is_valid_cli_profile(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Nome convencional de variável por provedor (pré-preenchido ao adicionar).
fn default_env(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAiApi => "OPENAI_ADMIN_KEY",
        Provider::AnthropicApi => "ANTHROPIC_ADMIN_KEY",
        Provider::ChatGpt => "CHATGPT_ACCESS_TOKEN",
        Provider::Claude => "CLAUDE_OAUTH_TOKEN",
        Provider::GeminiApi | Provider::VertexAi => "GOOGLE_CLOUD_ACCESS_TOKEN",
        Provider::AwsBedrock => "",
        Provider::AzureOpenAi => "AZURE_ACCESS_TOKEN",
        Provider::Cursor => "CURSOR_ADMIN_API_KEY",
        Provider::Copilot => "GITHUB_TOKEN",
        Provider::Windsurf => "WINDSURF_SERVICE_KEY",
        Provider::MiniMax => "MINIMAX_IO_API_KEY",
        Provider::Kimi => "KIMI_API_KEY",
        Provider::Qwen => "QWEN_API_KEY",
        Provider::Groq => "GROQ_API_KEY",
        Provider::Perplexity => "PERPLEXITY_API_KEY",
        Provider::Mistral => "MISTRAL_API_KEY",
        Provider::CloudflareAi => "CLOUDFLARE_AI_API_KEY",
        Provider::OllamaCloud => "OLLAMA_CLOUD_API_KEY",
        Provider::XiaomiMimo => "XIAOMI_MIMO_API_KEY",
        Provider::OpenCodeGo => "OPENCODE_GO_API_KEY",
        Provider::Qoder => "QODER_API_KEY",
        Provider::GeminiAi => "GEMINI_API_KEY",
        Provider::Grok => "X_GROK_API_KEY",
        Provider::NvidiaNim => "NVIDIA_NIM_API_KEY",
        Provider::Poolside => "POOLSIDE_API_KEY",
        Provider::Cline => "CLINE_API_KEY",
        Provider::Kiro => "KIRO_ACCESS_TOKEN",
        Provider::CodeBuddy => "CODEBUDDY_ACCESS_TOKEN",
        Provider::Zed => "ZED_ACCESS_TOKEN",
        Provider::AlibabaModelStudio => "DASHSCOPE_API_KEY",
        Provider::AlibabaCodingPlan => "BAILIAN_CODING_PLAN_API_KEY",
        Provider::AlibabaTokenPlan => "QWEN_API_KEY",
        Provider::AlibabaCloud => "ALIBABA_CLOUD_ACCESS_KEY_ID",
        Provider::Antigravity => "ANTIGRAVITY_ACCESS_TOKEN",
        Provider::Kimchi => "KIMCHI_API_KEY",
    }
}

fn credential_help(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAiApi => {
            "Chave de ADMINISTRAÇÃO da organização (Admin API), não uma chave de projeto comum."
        }
        Provider::AnthropicApi => {
            "Chave de administração (sk-ant-admin01-...) da Usage & Cost Admin API."
        }
        Provider::ChatGpt => {
            "Use o ACCESS token OAuth (não o refresh token nem o ID token). Interface interna do Codex: cobre as janelas do agente, NÃO a quota do chat web; pode mudar sem aviso."
        }
        Provider::Claude => {
            "Token OAuth de acesso. Interface não documentada oficialmente, sujeita a mudanças."
        }
        Provider::GeminiApi | Provider::VertexAi => {
            "Access token OAuth do Google Cloud (ex.: gcloud auth application-default print-access-token)."
        }
        Provider::AzureOpenAi => "Access token Azure com permissão de leitura de custos.",
        Provider::Cursor => {
            "Admin API Teams/Enterprise: variável CURSOR_ADMIN_API_KEY. Para a conta pessoal, pressione Tab e confirme importar a sessão do Cursor local. Não abre OAuth nem lê tokens sem confirmação."
        }
        Provider::Copilot => {
            "Token GitHub da CONTA, nunca o copilotToken de inferência. Cobrança (padrão): PAT clássico. Quota pessoal (opt-in): token GitHub com acesso ao endpoint interno; OAuth/PAT aceito depende do provedor."
        }
        Provider::Windsurf => {
            "Requer Enterprise com permissões Billing Read e Teams Read-Only."
        }
        Provider::AwsBedrock => "",
        Provider::Mistral => "A consulta de consumo exige Admin API Key, não chave de geração. Use MISTRAL_API_KEY com a chave administrativa ou configure admin_key_env nos parâmetros.",
        Provider::Grok => "Token de acesso xAI da equipe (Management API ou CLI). Padrão Management API: Management Key + Team ID. Opt-in cli: token de sessão do Grok CLI. Não confundir com chave da API social X.",
        Provider::Perplexity => "PERPLEXITY_API_KEY é mantida. Sonar não expõe saldo por essa chave; Computer Enterprise Analytics é uma consulta distinta, opcional nos parâmetros.",
        Provider::GeminiAi => "Padrão: chave Gemini/AI Studio — esta chave NÃO consulta quota (use Gemini CLI/CodeAssist nos parâmetros). Opt-in cli: token de sessão do Gemini CLI.",
        Provider::Cline => "Chave da Cline API ou token de conta (CLINE_API_KEY). Consulta saldo Cline; BYOK deve ser acompanhado no provedor da chave. Organização opcional nos parâmetros.",
        Provider::Kimi => "KIMI_API_KEY (Moonshot/Kimi). Padrão: saldo da conta internacional. Opt-in coding: aceita a chave de plataforma OU um access token OAuth (informe o nome em token_env); a chave tem precedência.",
        Provider::Qoder => "QODER_API_KEY aceita PAT (pt-..., trocado por job token) ou um token Bearer/job já existente (jt-.../device). Escolha o tipo de credencial nos parâmetros.",
        Provider::OpenCodeGo => "OPENCODE_GO_API_KEY da conta; a própria chave do usuário autentica a consulta interna.",
        Provider::Groq => "GROQ_API_KEY da conta; a própria chave do usuário autentica a consulta interna.",
        Provider::OllamaCloud => "OLLAMA_CLOUD_API_KEY da conta (Bearer); uso em GET ollama.com/api/usage e plano em POST /api/me. Não é necessário importar cookies.",
        Provider::XiaomiMimo => "Saldo/Token Plan: configure platform_session_env com a sessão web platform.xiaomimimo.com. O código nativo entrega chave de inferência, não cookie. session_env é separado: quota semanal do MiMo Desktop.",
        Provider::Kiro => "Access token Kiro/CodeWhisperer (não refresh token). Origem via auth_method: builder-id (padrão), idc, google, github, import, api_key ou external_idp.",
        Provider::CodeBuddy => "Access token de sessão do CodeBuddy (cn: copilot.tencent.com; intl: www.codebuddy.ai).",
        Provider::Zed => "ZED_ACCESS_TOKEN (cloud access token do fluxo de login nativo zed.dev). Informe também o user_id numérico — é enviado como `Authorization: <user_id> <token>`.",
        Provider::AlibabaModelStudio => "API key Alibaba Model Studio/DashScope. Pode apontar para QWEN_API_KEY. Selecione a região correspondente à conta; não é login OAuth Qwen Code.",
        Provider::AlibabaCodingPlan => "Chave da assinatura Alibaba Coding Plan (normalmente sk-sp-). Selecione China ou internacional; é diferente de uma API key padrão DashScope.",
        Provider::AlibabaTokenPlan => "A quota Token Plan consulta, por padrão, o perfil dedicado do `bl` (`params.cli_profile`, configurado pelo F2). `params.session_env` (Cookie de home.qwencloud.com) é override opcional. QWEN_API_KEY autentica apenas inferência e não é enviada à consulta.",
        Provider::AlibabaCloud => "Informe o NOME da variável com AccessKey ID da conta RAM de faturamento. Em Parâmetros informe a variável AccessKey Secret; os valores nunca entram no TOML.",
        Provider::Antigravity => "Sessão Google do Antigravity, distinta da API key Gemini. Pressione Tab para login nativo; o projeto Code Assist e as janelas de quota serão consultados automaticamente.",
        Provider::Kimchi => "KIMCHI_API_KEY da conta Kimchi; consulta créditos e orçamentos em llm.kimchi.dev, sem geração.",
        _ => "API key/PAT: informe apenas o nome da variável exportada. A chave de geração pode não permitir consultar consumo; confira a limitação do relatório.",
    }
}

/// Instruções de registro do aplicativo OAuth do próprio usuário (URLs oficiais).
/// Chamada apenas para provedores com `supports_login` verdadeiro.
fn oauth_registration_help(provider: Provider) -> &'static str {
    match provider {
        Provider::GeminiApi | Provider::VertexAi => {
            "Registre um client ID de app Desktop em https://console.cloud.google.com/apis/credentials (tela de consentimento OAuth + cliente Desktop; o login usa PKCE com loopback local)."
        }
        Provider::AzureOpenAi => {
            "Registre um app público em https://portal.azure.com > Microsoft Entra > Registros de aplicativo, habilite o fluxo de código de dispositivo e conceda permissão delegada de gerenciamento/leitura do Azure."
        }
        _ => "Registre um aplicativo OAuth próprio no provedor e informe o client ID.",
    }
}

/// Motivo da ausência de login integrado (modo variável de ambiente segue
/// disponível normalmente). AWS usa o fluxo próprio do CLI (perfil + F2);
fn oauth_unsupported_reason(provider: Provider) -> Option<&'static str> {
    match provider {
        Provider::OpenAiApi => Some("a Admin API da OpenAI aceita apenas chaves de administração."),
        Provider::AnthropicApi => {
            Some("a Usage & Cost Admin API aceita apenas chaves de administração.")
        }
        Provider::Windsurf => Some("o Windsurf exige service key Enterprise; não há fluxo OAuth."),
        _ => None,
    }
}

/// Opções não secretas do fluxo de login nativo, editadas no passo Credencial
/// ANTES de iniciar o login e gravadas em `OAuthConfig.options` (apenas
/// region/auth_method/start_url — nunca tokens). Pré-carregadas da config
/// existente ao editar uma conta já autenticada.
fn native_option_fields(provider: Provider, existing: Option<&OAuthConfig>) -> Vec<Field> {
    let get = |key: &str| existing.and_then(|o| o.options.get(key)).cloned();
    let text = |key: &'static str, label: &'static str, help: &'static str| {
        let mut f = Field::text(key, label, help, false);
        if let Some(v) = get(key) {
            f.kind = FieldKind::Text(v);
        }
        f
    };
    let choice = |key: &'static str,
                  label: &'static str,
                  help: &'static str,
                  options: &'static [(&'static str, &'static str)]| {
        let mut f = Field::choice(key, label, help, options);
        if let Some(v) = get(key) {
            if let Some(pos) = options.iter().position(|(value, _)| *value == v) {
                if let FieldKind::Choice { selected, .. } = &mut f.kind {
                    *selected = pos;
                }
            }
        }
        f
    };
    match provider {
        Provider::Cursor => vec![text("database_path", "Banco local Cursor (opcional)", "Caminho completo de state.vscdb; vazio usa Cursor instalado. Nunca informe o token neste campo.")],
        Provider::Kiro => vec![
            choice(
                "auth_method",
                "Método de autenticação",
                "builder-id (padrão): AWS Builder ID. idc: IAM Identity Center (usa região/URL abaixo).",
                &[("builder-id", "AWS Builder ID"), ("idc", "IAM Identity Center")],
            ),
            text(
                "region",
                "Região (opcional)",
                "Região AWS do Identity Center (ex.: us-east-1); vazio usa o padrão do fluxo.",
            ),
            text(
                "start_url",
                "URL de início IDC (opcional)",
                "URL do portal IAM Identity Center; usada apenas no método idc.",
            ),
        ],
        Provider::CodeBuddy => vec![choice(
            "region",
            "Região",
            "cn (padrão): copilot.tencent.com. intl: www.codebuddy.ai.",
            &[("cn", "China"), ("intl", "Internacional")],
        )],
        Provider::Qoder => vec![choice(
            "region",
            "Região",
            "intl (padrão): openapi.qoder.sh. cn: openapi.qoder.com.cn.",
            &[("intl", "Internacional"), ("cn", "China")],
        )],
        Provider::XiaomiMimo => vec![choice(
            "region",
            "Região",
            "Região do serviço MiMo: sgp (padrão), cn, ams, ru ou in.",
            &[
                ("sgp", "Singapura"),
                ("cn", "China"),
                ("ams", "Amsterdã"),
                ("ru", "Rússia"),
                ("in", "Índia"),
            ],
        )],
        _ => Vec::new(),
    }
}

/// Chaves de params controladas pelo login nativo: ocultas no passo Parâmetros
/// em modo nativo (nunca divergem da identidade autenticada) e preenchidas
/// pelos metadados públicos devolvidos pelo login (ou preservadas da conta em
/// edição sem novo login). Seguem a projeção pública de `oauth::account_params`.
fn native_param_keys(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Qwen => &["product"],
        Provider::Cursor => &["query_mode", "device_id", "user_id"],
        Provider::ChatGpt => &["account_id"],
        Provider::Copilot => &["query_mode", "mode", "org", "billing"],
        Provider::Kimi => &["product", "token_env", "device_id"],
        Provider::Grok => &[
            "product",
            "session_env",
            "email",
            "user_id",
            "team_id",
            "management_key_env",
        ],
        Provider::GeminiAi => &["product", "session_env", "project_id"],
        Provider::Qoder => &["region", "credential_type"],
        Provider::CodeBuddy => &["region"],
        Provider::Kiro => &["auth_method", "profile_arn", "region"],
        Provider::Zed => &["user_id", "system_id"],
        Provider::XiaomiMimo => &["region", "auth_method"],
        _ => &[],
    }
}

/// Chaves de params gerenciadas pelo assistente por provedor (inclui `org` oculta
/// e `profile` do AWS): removidas do clone de edição e regravadas a partir dos
/// campos. Chaves exclusivas do login nativo (region/auth_method etc.) também
/// são removidas no clone — em modo nativo são preenchidas pelos metadados do
/// login; em modo env ficam vazias até nova autenticação (mudança de identidade).
fn managed_keys(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Qwen => &["product"],
        Provider::OpenAiApi => &["project_ids", "models"],
        Provider::AnthropicApi => &["workspace_ids", "models"],
        Provider::ChatGpt => &["account_id"],
        Provider::GeminiApi | Provider::VertexAi => &["project_id"],
        Provider::AwsBedrock => &["profile"],
        Provider::AzureOpenAi => &["subscription_id", "resource_id"],
        Provider::Copilot => &["mode", "org", "billing", "query_mode"],
        Provider::Cursor => &["query_mode", "device_id", "user_id"],
        Provider::Mistral => &["admin_key_env"],
        Provider::Kimi => &["product", "token_env", "device_id"],
        Provider::XiaomiMimo => &[
            "session_env",
            "platform_session_env",
            "region",
            "auth_method",
        ],
        Provider::Grok => &[
            "product",
            "session_env",
            "email",
            "user_id",
            "team_id",
            "management_key_env",
        ],
        Provider::Perplexity => &["product", "analytics_key_env"],
        Provider::Cline => &["organization_id"],
        Provider::GeminiAi => &["product", "session_env", "project_id"],
        Provider::Qoder => &["region", "credential_type"],
        Provider::Kiro => &["auth_method", "profile_arn", "region"],
        Provider::CodeBuddy => &["region"],
        Provider::Zed => &["user_id", "system_id"],
        Provider::Antigravity => &["project_id"],
        Provider::AlibabaModelStudio => &["region"],
        Provider::AlibabaCodingPlan => &["region", "session_env"],
        Provider::AlibabaTokenPlan => &["session_env"],
        Provider::AlibabaCloud => &[
            "region",
            "secret_env",
            "security_token_env",
            "billing_cycle",
        ],
        _ => &[],
    }
}

fn default_fields(provider: Provider, existing: Option<&Account>) -> Vec<Field> {
    let get = |key: &str| existing.and_then(|a| a.params.get(key)).cloned();
    let text = |key: &'static str, label: &'static str, help: &'static str, required: bool| {
        let mut f = Field::text(key, label, help, required);
        if let Some(v) = get(key) {
            f.kind = FieldKind::Text(v);
        }
        f
    };
    let choice = |key: &'static str,
                  label: &'static str,
                  help: &'static str,
                  options: &'static [(&'static str, &'static str)]| {
        let mut f = Field::choice(key, label, help, options);
        if let Some(v) = get(key) {
            if let Some(pos) = options.iter().position(|(value, _)| *value == v) {
                if let FieldKind::Choice { selected, .. } = &mut f.kind {
                    *selected = pos;
                }
            }
        }
        f
    };
    match provider {
        Provider::OpenAiApi => vec![
            text(
                "project_ids",
                "IDs de projetos",
                "Opcional; lista separada por vírgula para filtrar custos e uso.",
                false,
            ),
            text(
                "models",
                "Modelos",
                "Opcional; lista separada por vírgula para filtrar o uso.",
                false,
            ),
        ],
        Provider::AnthropicApi => vec![
            text(
                "workspace_ids",
                "IDs de workspaces",
                "Opcional; lista separada por vírgula para filtrar custos e uso.",
                false,
            ),
            text(
                "models",
                "Modelos",
                "Opcional; lista separada por vírgula para filtrar o uso.",
                false,
            ),
        ],
        Provider::ChatGpt => vec![text(
            "account_id",
            "ID da conta",
            "Opcional; cabeçalho chatgpt-account-id para conta de equipe.",
            false,
        )],
        Provider::Claude | Provider::Cursor | Provider::Windsurf | Provider::AwsBedrock => {
            Vec::new()
        }
        Provider::GeminiApi | Provider::VertexAi => vec![text(
            "project_id",
            "ID do projeto GCP",
            "Obrigatório; letras, números e - _ . :",
            true,
        )],
        Provider::AzureOpenAi => vec![
            text(
                "subscription_id",
                "ID da assinatura",
                "Obrigatório; assinatura Azure dona do recurso.",
                true,
            ),
            text(
                "resource_id",
                "ID ARM do recurso",
                "Obrigatório; caminho completo /subscriptions/{sub}/resourceGroups/{rg}/providers/Microsoft.CognitiveServices/accounts/{nome}.",
                true,
            ),
        ],
        Provider::Copilot => vec![
            choice(
                "query_mode",
                "Modo de consulta",
                "billing (padrão): APIs de cobrança documentadas. quota: quota pessoal (interface interna; exige token OAuth/PAT do GitHub, NÃO o copilotToken interno).",
                &[
                    ("billing", "Cobrança documentada (padrão)"),
                    ("quota", "Quota pessoal (opt-in, interface interna)"),
                ],
            ),
            choice(
                "mode",
                "Escopo",
                "user: cobrança pessoal; org: cobrança da organização (requer admin). Sem efeito no modo quota.",
                &[
                    ("user", "Cobrança pessoal do usuário autenticado"),
                    ("org", "Cobrança da organização (requer admin)"),
                ],
            ),
            text(
                "org",
                "Organização",
                "Obrigatório no modo org; slug da organização (letras, números e hífens).",
                true,
            ),
            choice(
                "billing",
                "Produto",
                "premium_requests (padrão) ou ai_credits.",
                &[
                    ("premium_requests", "Solicitações premium"),
                    ("ai_credits", "Créditos de IA"),
                ],
            ),
        ],
        Provider::Mistral => vec![text("admin_key_env", "Variável administrativa alternativa", "Opcional: nome da variável com Admin API Key. Vazio usa a variável da etapa Credencial, que deve conter chave administrativa.", false)],
        Provider::Kimi => vec![
            choice(
                "product",
                "Produto",
                "Padrão: saldo da conta internacional Kimi/Moonshot. coding: opt-in para a quota do plano Kimi for Coding (interface interna, sujeita a mudanças).",
                &[
                    ("default", "Saldo da conta internacional (padrão)"),
                    ("coding", "Kimi for Coding — quota do plano (opt-in, interno)"),
                ],
            ),
            text(
                "token_env",
                "Variável do token OAuth (opcional)",
                "Opcional; nome da variável com access token OAuth Kimi. Usado somente se a chave da etapa Credencial estiver ausente; a API key tem precedência, como no 9router.",
                false,
            ),
        ],
        Provider::Grok => vec![
            choice(
                "product",
                "Produto",
                "Padrão: Management API xAI (saldo/cobrança, exige Team ID). cli: opt-in para a quota do Grok CLI (interface interna) com token de sessão.",
                &[
                    ("management", "Management API (padrão)"),
                    ("cli", "Grok CLI (opt-in, interface interna)"),
                ],
            ),
            text(
                "session_env",
                "Variável do token de sessão",
                "Nome (não valor) da variável com o access token do Grok CLI; o padrão é GROK_CLI_ACCESS_TOKEN — informe outro nome apenas se necessário. Usado somente no produto cli.",
                false,
            ),
            text(
                "email",
                "E-mail (opcional)",
                "Opcional no produto cli; enviado como cabeçalho x-email.",
                false,
            ),
            text(
                "user_id",
                "User ID (opcional)",
                "Opcional no produto cli; enviado como cabeçalho x-userid.",
                false,
            ),
            text("team_id", "Team ID xAI", "Obrigatório na Management API, disponível nas configurações da equipe xAI.", true),
            text("management_key_env", "Variável de Management Key alternativa", "Opcional: nome da variável com Management Key, não o valor. Vazio usa a variável da etapa Credencial.", false),
        ],
        Provider::Perplexity => vec![
            text("product", "Produto (opcional)", "Digite computer somente para Computer Enterprise Analytics. Vazio mantém Perplexity/Sonar com limitação explícita.", false),
            text("analytics_key_env", "Variável Analytics alternativa", "Opcional: variável com chave Computer Analytics. Não consulta consumo Sonar.", false),
        ],
        Provider::Cline => vec![text("organization_id", "Organização Cline (opcional)", "Vazio consulta a conta pessoal. Informe o ID da organização para consultar seu saldo, com a permissão correspondente.", false)],
        Provider::GeminiAi => vec![
            choice(
                "product",
                "Produto",
                "A chave de API Gemini/AI Studio NÃO tem consulta de quota. cli: opt-in para a quota do Gemini CLI (CodeAssist, interface interna) com token de sessão.",
                &[
                    ("default", "Chave de API (sem consulta de quota)"),
                    ("cli", "Gemini CLI / CodeAssist (opt-in, interno)"),
                ],
            ),
            text(
                "session_env",
                "Variável do token de sessão",
                "Nome (não valor) da variável com o access token do Gemini CLI; o padrão é GEMINI_CLI_ACCESS_TOKEN — informe outro nome apenas se necessário. Usado somente no produto cli.",
                false,
            ),
            text(
                "project_id",
                "ID do projeto GCP (opcional)",
                "Opcional no produto cli; projeto CodeAssist. Vazio tenta resolução automática.",
                false,
            ),
        ],
        Provider::Qoder => vec![
            choice(
                "region",
                "Região",
                "intl (padrão): openapi.qoder.sh. cn: openapi.qoder.com.cn.",
                &[
                    ("intl", "Internacional (openapi.qoder.sh)"),
                    ("cn", "China (openapi.qoder.com.cn)"),
                ],
            ),
            choice(
                "credential_type",
                "Tipo de credencial",
                "auto (padrão): detecta o prefixo pt-. pat: Personal Access Token (trocado por job token). bearer: token Bearer/job já existente.",
                &[
                    ("auto", "Automático (detecta pt-)"),
                    ("pat", "Personal Access Token"),
                    ("bearer", "Bearer/job token existente"),
                ],
            ),
        ],
        Provider::Kiro => vec![
            choice(
                "auth_method",
                "Método de autenticação",
                "Origem do access token: builder-id (padrão), idc, google, github, import, api_key ou external_idp.",
                &[
                    ("builder-id", "AWS Builder ID (padrão)"),
                    ("idc", "IAM Identity Center"),
                    ("google", "Google"),
                    ("github", "GitHub"),
                    ("import", "Importado"),
                    ("api_key", "API key"),
                    ("external_idp", "IdP externo"),
                ],
            ),
            text(
                "profile_arn",
                "ARN do perfil CodeWhisperer (opcional)",
                "Perfil da sua conta; opcional no fluxo legado. Permite consultar créditos atuais no endpoint regional. Nunca use ARN de outra conta.",
                false,
            ),
        ],
        Provider::CodeBuddy => vec![choice(
            "region",
            "Região",
            "cn (padrão): copilot.tencent.com (cabeçalhos CLI). intl: www.codebuddy.ai (cabeçalhos IDE).",
            &[
                ("cn", "China (copilot.tencent.com)"),
                ("intl", "Internacional (www.codebuddy.ai)"),
            ],
        )],
        Provider::Zed => vec![
            text(
                "user_id",
                "ID numérico do usuário Zed",
                "Obrigatório; enviado como \"Authorization: <user_id> <access_token>\".",
                true,
            ),
            text(
                "system_id",
                "System ID (opcional)",
                "Opcional; enviado como cabeçalho x-zed-system-id quando definido.",
                false,
            ),
        ],
        Provider::XiaomiMimo => vec![
            text("platform_session_env", "Variável da sessão da plataforma (opcional)", "Nome da variável com Cookie de platform.xiaomimimo.com (api-platform_serviceToken e userId). Lê saldo/plano mensal; não cole cookies neste campo.", false),
            text(
                "session_env",
                "Variável do cookie de sessão (opcional)",
                "Nome da variável com o cookie de sessão da conta Xiaomi (ex.: serviceToken=...), nunca o valor. Tem prioridade sobre a credencial principal.",
                false,
            ),
            choice(
                "region",
                "Região",
                "Região do serviço MiMo: sgp (padrão), cn, ams, ru ou in.",
                &[
                    ("sgp", "Singapura (padrão)"),
                    ("cn", "China"),
                    ("ams", "Amsterdã"),
                    ("ru", "Rússia"),
                    ("in", "Índia"),
                ],
            ),
        ],
        Provider::Antigravity => vec![text("project_id", "Projeto Code Assist (opcional)", "Vazio tenta obter o projeto pela sessão Antigravity. Não é obrigatório informar o projeto usado por uma API key Gemini.", false)],
        Provider::AlibabaModelStudio => vec![choice(
            "region", "Região Alibaba", "Use a região onde sua conta/chave foi criada; China e internacional possuem endpoints distintos.",
            &[("intl", "Internacional"), ("cn", "China continental")],
        )],
        Provider::AlibabaCodingPlan => vec![
            choice("region", "Região Alibaba", "Use a região da assinatura; a credencial não será enviada automaticamente para outra região.", &[("intl", "Internacional"), ("cn", "China continental")]),
            text("session_env", "Variável da sessão Alibaba (opcional)", "Se a API exigir login no console, informe o NOME da variável contendo o Cookie da sua sessão. Substitui a API key na consulta; nunca cole o cookie aqui.", false),
        ],
        Provider::AlibabaTokenPlan => vec![text("session_env", "Variável do Cookie Qwen Cloud (opcional)", "Vazio = quota via perfil dedicado do `bl` (`params.cli_profile`). Se preferir a rota manual, informe o NOME da variável contendo o Cookie de home.qwencloud.com; nunca cole o Cookie aqui.", false)],
        Provider::AlibabaCloud => vec![
            choice("region", "Conta de faturamento", "Internacional ou China continental; corresponde à conta de cobrança Alibaba.", &[("intl", "Internacional"), ("cn", "China continental")]),
            text("secret_env", "Variável AccessKey Secret", "Opcional: padrão ALIBABA_CLOUD_ACCESS_KEY_SECRET. Informe apenas o nome da variável, nunca o segredo.", false),
            text("security_token_env", "Variável STS Security Token (opcional)", "Somente para credenciais temporárias STS; deixe vazio para AccessKey permanente.", false),
            text("billing_cycle", "Fatura mensal (opcional)", "YYYY-MM, por exemplo 2026-09. Vazio consulta apenas saldo. Fatura agrega todos os produtos da conta cloud, não somente AI.", false),
        ],
        _ => Vec::new(),
    }
}

/// Validações alinhadas aos adaptadores (path_id / slug / azure_resource_id).
fn validate_param(key: &str, value: &str, params: &BTreeMap<String, String>) -> Result<(), String> {
    match key {
        "billing_cycle" => {
            if value.len() == 7
                && chrono::NaiveDate::parse_from_str(&format!("{value}-01"), "%Y-%m-%d").is_ok()
            {
                Ok(())
            } else {
                Err("Informe o ciclo no formato YYYY-MM.".into())
            }
        }
        k if k.ends_with("_env") => {
            if valid_env_name(value) {
                Ok(())
            } else {
                Err("Informe apenas o nome da variável de ambiente, nunca o valor.".into())
            }
        }
        "team_id" | "organization_id" | "user_id" => {
            if !value.is_empty()
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            {
                Ok(())
            } else {
                Err("Identificador inválido: use letras, números, hífen ou sublinhado.".into())
            }
        }
        "system_id" => {
            if value.is_empty()
                || value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            {
                Ok(())
            } else {
                Err("System ID inválido: use letras, números, hífen ou sublinhado.".into())
            }
        }
        "email" => {
            // Checagem leve: precisa ter '@' sem espaços/controle; o servidor valida o resto.
            if value.is_empty()
                || (value.contains('@')
                    && value.chars().all(|c| !c.is_control() && !c.is_whitespace()))
            {
                Ok(())
            } else {
                Err("E-mail inválido.".into())
            }
        }
        "profile_arn" => {
            if value.is_empty()
                || (value.starts_with("arn:")
                    && value.chars().all(|c| {
                        c.is_ascii_alphanumeric() || matches!(c, ':' | '/' | '-' | '_' | '.')
                    }))
            {
                Ok(())
            } else {
                Err("ARN inválido: deve começar com \"arn:\" e usar apenas caracteres AWS.".into())
            }
        }
        "product" => {
            if value == "computer" {
                Ok(())
            } else {
                Err(
                    "Use computer para Computer Analytics, ou deixe vazio para Perplexity/Sonar."
                        .into(),
                )
            }
        }
        "project_id" | "subscription_id" => {
            if value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
            {
                Ok(())
            } else {
                Err(format!(
                    "params.{key} contém caracteres inválidos para identificador de recurso."
                ))
            }
        }
        "resource_id" => {
            let subscription = params
                .get("subscription_id")
                .map(String::as_str)
                .unwrap_or_default();
            let segments: Vec<&str> = value.split('/').collect();
            let valid = !subscription.is_empty()
                && value.starts_with(&format!("/subscriptions/{subscription}/resourceGroups/"))
                && value.contains("/providers/Microsoft.CognitiveServices/accounts/")
                && !value.ends_with('/')
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
                && segments.iter().skip(1).all(|s| !s.is_empty());
            if valid {
                Ok(())
            } else {
                Err("resource_id deve ser o ID ARM completo dentro da assinatura informada: /subscriptions/{sub}/resourceGroups/{rg}/providers/Microsoft.CognitiveServices/accounts/{nome}.".into())
            }
        }
        "org" => {
            if !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                Ok(())
            } else {
                Err("Organização inválida: use apenas letras, números e hífens.".into())
            }
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn account(name: &str, env: &str) -> Account {
        Account {
            name: name.into(),
            provider: Provider::OpenAiApi,
            credential_env: Some(env.into()),
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
        }
    }

    fn wizard_with(name: &str, env: &str, interval: &str) -> Wizard {
        let config = Config {
            accounts: vec![account(name, env)],
            ..Config::default()
        };
        let mut wiz = Wizard::new(&config, Some(0));
        wiz.interval = interval.into();
        wiz
    }

    #[test]
    fn parse_interval_blank_means_inherit_global() {
        // Campo vazio é o estado "sem override": o intervalo global vale.
        let wiz = wizard_with("OpenAI", "OPENAI_ADMIN_KEY", "");
        assert_eq!(wiz.parse_interval().unwrap(), None);
    }

    #[test]
    fn parse_interval_preloads_existing_override() {
        // Editar uma conta com override deve pré-carregar o valor existente,
        // não o global — para que o usuário veja o que está prestes a apagar.
        let mut acc = account("OpenAI", "OPENAI_ADMIN_KEY");
        acc.refresh_seconds = Some(120);
        let config = Config {
            accounts: vec![acc],
            ..Config::default()
        };
        let wiz = Wizard::new(&config, Some(0));
        assert_eq!(wiz.interval, "120");
    }

    #[test]
    fn parse_interval_rejects_out_of_range() {
        let below = wizard_with("OpenAI", "OPENAI_ADMIN_KEY", "29");
        assert!(below.parse_interval().is_err());
        let above = wizard_with("OpenAI", "OPENAI_ADMIN_KEY", "99999");
        assert!(above.parse_interval().is_err());
        let ok = wizard_with("OpenAI", "OPENAI_ADMIN_KEY", "90");
        assert_eq!(ok.parse_interval().unwrap(), Some(90));
    }

    #[test]
    fn build_config_writes_override_to_account_without_touching_global() {
        // Garantia de invariante: o assistente grava apenas `account.refresh_seconds`,
        // nunca o `config.refresh_seconds` global. O global permanece como
        // fallback para as demais contas.
        let config = Config {
            accounts: vec![account("OpenAI", "OPENAI_ADMIN_KEY")],
            ..Config::default()
        };
        let mut wiz = Wizard::new(&config, Some(0));
        wiz.interval = "60".into();
        let built = wiz.build_config().unwrap();
        assert_eq!(built.refresh_seconds, 300);
        assert_eq!(built.accounts[0].refresh_seconds, Some(60));
    }

    #[test]
    fn build_config_blank_interval_clears_existing_override() {
        // Editar uma conta e deixar o campo em branco apaga o override
        // individual e devolve a conta ao regime global.
        let mut acc = account("OpenAI", "OPENAI_ADMIN_KEY");
        acc.refresh_seconds = Some(45);
        let config = Config {
            accounts: vec![acc],
            ..Config::default()
        };
        let mut wiz = Wizard::new(&config, Some(0));
        wiz.interval = "".into();
        let built = wiz.build_config().unwrap();
        assert_eq!(built.refresh_seconds, 300);
        assert_eq!(built.accounts[0].refresh_seconds, None);
    }
    #[test]
    fn bailian_login_uses_and_persists_account_profile() {
        let config = Config {
            accounts: vec![Account {
                name: "Qwen Cloud".into(),
                provider: Provider::AlibabaTokenPlan,
                credential_env: None,
                oauth: None,
                params: BTreeMap::from([("cli_profile".into(), "qwen-personal".into())]),
                refresh_seconds: None,
            }],
            ..Config::default()
        };
        let mut wiz = Wizard::new(&config, Some(0));
        wiz.step = Step::Credential;
        let action = wiz.handle(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
        assert!(matches!(
            action,
            Action::BailianConsoleLogin { console_site, profile }
                if console_site == "international" && profile == "qwen-personal"
        ));
        wiz.bailian_login_finished();
        assert_eq!(wiz.step, Step::Params);
        let built = wiz.build_config().unwrap();
        assert_eq!(
            built.accounts[0]
                .params
                .get("cli_profile")
                .map(String::as_str),
            Some("qwen-personal")
        );
        assert!(built.accounts[0].credential_env.is_none());
    }
}
