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
use cuotai_core::account_edit as edit;
use cuotai_core::login::{self, LoginMode};
use cuotai_core::oauth::{requires_client_registration, supports_login, OAuthConfig};
use cuotai_core::{Account, Config, Provider};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
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
    /// The wizard finished and produced a new configuration that differs
    /// from `editing`-based base by exactly one upsert. `edit` carries the
    /// index of the affected account (`Some(i)` when editing, `None` when
    /// appending a new account).
    Save {
        config: Config,
        edit: Option<usize>,
    },
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
    /// RF4: the file on disk changed and the user accepted the
    /// conflict-recovery modal's "reload" action. The TUI must read
    /// the file, refresh the dashboard's view of accounts, and keep
    /// the wizard's draft intact so the user can re-validate against
    /// the freshly-loaded schema.
    ConflictReload,
    /// RF4: the user dismissed the conflict modal without reloading.
    /// The wizard stays open; the user can edit the draft and retry
    /// the save.
    ConflictDismiss,
}

const MAX_INTERVAL: u64 = edit::MAX_REFRESH_SECONDS;

/// Effective minimum interval for the currently selected provider. The
/// global floor (30s) covers every provider; metered providers raise
/// the floor to the cost-guard (300s) so the wizard agrees with
/// `Config::validate`. Returning the larger of the two keeps the
/// wizard silent for free providers and assertive for paid ones.
fn min_interval_for(provider: Provider) -> u64 {
    if provider.is_metered() {
        Provider::METERED_MIN_REFRESH_SECONDS
    } else {
        edit::MIN_REFRESH_SECONDS
    }
}

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
    /// Mensagem do conflito externo detectado ao tentar salvar
    /// (RF4). Enquanto estiver presente, o assistente exibe um
    /// diálogo modal oferecendo `r` para recarregar o arquivo do
    /// disco (mantendo o rascunho) ou `Esc` para cancelar.
    conflict: Option<String>,
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
        // Stored credential value comes from core: AWS uses params["profile"],
        // AlibabaTokenPlan uses params["cli_profile"], everything else uses
        // credential_env. Brand-new accounts prefilled with the conventional
        // env-var name (or empty for non-env kinds).
        let credential = match existing {
            Some(a) => edit::stored_credential(a),
            None => edit::suggested_credential(provider).to_string(),
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
            conflict: None,
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

    /// Display a transient success notice (OAuth/SSO, file reload).
    /// Cleared by the next key press, same lifetime as other notices.
    pub(crate) fn set_notice(&mut self, message: String) {
        self.error = None;
        self.conflict = None;
        self.notice = Some(message);
    }

    /// Enter the conflict-recovery modal: the file on disk changed
    /// since we last loaded or saved it. The wizard keeps its draft
    /// (name/credential/fields/interval) intact; the caller can offer
    /// the user a chance to reload via [`Wizard::take_conflict`] and
    /// rebuild the runtime against the freshly-loaded file.
    pub(crate) fn set_conflict(&mut self, message: String) {
        self.conflict = Some(message);
        self.error = None;
    }

    /// Are we currently in the conflict modal?
    #[allow(dead_code)]
    pub(crate) fn has_conflict(&self) -> bool {
        self.conflict.is_some()
    }

    /// Take ownership of the conflict message (for rendering). The
    /// modal stays open until [`Wizard::clear_conflict`] is called
    /// (typically after the user accepted reload or cancelled).
    #[allow(dead_code)]
    pub(crate) fn conflict(&self) -> Option<&str> {
        self.conflict.as_deref()
    }

    /// Dismiss the conflict modal without reloading. The wizard
    /// stays open and the user can edit the draft further before
    /// retrying the save.
    pub(crate) fn clear_conflict(&mut self) {
        self.conflict = None;
    }

    /// Replace the wizard's `base` snapshot with the freshly-loaded
    /// configuration. Called by the TUI after a successful reload:
    /// the draft (provider/name/credential/fields/interval) is kept
    /// untouched, but the schema the draft validates against is now
    /// the disk version.
    pub(crate) fn replace_base(&mut self, config: &Config) {
        self.base = config.clone();
    }

    /// Borrowed view of the wizard's editing identity for callers
    /// that need a `(Provider, &str)` shape without taking ownership
    /// of the wizard's name. Returns `None` when the wizard is
    /// creating a brand-new account.
    pub(crate) fn editing_identity_borrowed(&self) -> Option<(Provider, &str)> {
        self.editing?;
        Some((self.provider, self.name.trim()))
    }

    fn provider_at_sel(&self) -> Provider {
        Provider::ALL[self.provider_sel]
    }

    /// Campos visíveis: decisão única no core via `edit::visible_field_indices`,
    /// que conhece o schema do provedor, o modo nativo, os params de login e as
    /// seleções de Choice atuais. O TUI apenas repete o resultado para indexar
    /// os widgets de Field.
    fn visible_fields(&self) -> Vec<usize> {
        let selections = self.choice_selections();
        let visibility = edit::FieldVisibility {
            native_login: self.native_login(),
            login_params: &self.oauth_login_params,
            choice_selections: &selections,
        };
        edit::visible_field_indices(self.provider, &visibility)
    }

    /// Snapshot das seleções de Choice atuais. Core precisa desse mapa para
    /// avaliar regras condicionais (`org` aparece quando `mode == 1`, etc.);
    /// construímos a partir dos widgets do wizard para que UI e backend
    /// concordem sempre.
    fn choice_selections(&self) -> BTreeMap<String, usize> {
        let mut map = BTreeMap::new();
        for field in &self.fields {
            if let FieldKind::Choice { selected, .. } = &field.kind {
                map.insert(field.key.to_string(), *selected);
            }
        }
        map
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
                    self.credential = edit::default_env(selected).to_string();
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

    /// Whether the credential step offers a Tab → OAuth toggle. The core
    /// `credential_modes` table is the single source of truth: AWS and
    /// Token Plan never expose OAuth, every other provider with
    /// `supports_login` does.
    fn credential_supports_oauth(&self) -> bool {
        edit::credential_modes(self.provider).oauth
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
                if !secret.is_empty() && !edit::valid_env_name(secret) {
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
        // RF4: the conflict-recovery modal takes over key handling
        // when active. `r` reloads from disk (keeping the draft),
        // `Esc` dismisses the modal and lets the user keep editing
        // the wizard. All other keys are ignored so the user does
        // not accidentally edit the draft behind the modal.
        if self.conflict.is_some() {
            return match key.code {
                KeyCode::Char('r') | KeyCode::Char('R') => Action::ConflictReload,
                KeyCode::Esc => Action::ConflictDismiss,
                _ => Action::Continue,
            };
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
                        Ok(config) => Action::Save {
                            config,
                            edit: self.editing,
                        },
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
                if self.step == Step::Credential && self.credential_supports_oauth() =>
            {
                self.oauth_mode = true;
                self.oauth_idx = 0;
                self.invalidate_oauth();
                self.error = None;
            }
            (KeyCode::F(2), _)
                if self.step == Step::Credential
                    && wizard_offers_mode(self.provider, login::MODE_AWS_SSO) =>
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
                if self.step == Step::Credential
                    && wizard_offers_mode(self.provider, login::MODE_BAILIAN_CONSOLE) =>
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
                let profile = edit::ensure_cli_profile(&mut params);
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
    /// Toda a política de credencial / nome / visibilidade vive em core;
    /// o wizard apenas invoca os helpers e devolve a mensagem.
    fn validate_step(&self) -> Result<(), String> {
        let result = match self.step {
            Step::Name => edit::validate_name(&self.name, &self.base, self.editing).map(|_| ()),
            Step::Credential => {
                // Credential-form policy lives entirely in core; the wizard
                // only forwards the current text and the active OAuth state.
                match edit::credential_kind(self.provider) {
                    edit::CredentialKind::AwsProfile => {
                        edit::apply_aws_profile(self.provider, &self.credential)
                    }
                    edit::CredentialKind::None => {
                        // Token-plan quota uses the dedicated `bl` profile;
                        // the credential text slot is intentionally empty
                        // and must not be sent to `resolve_credential_env`.
                        let oauth = self
                            .oauth_authorized
                            .as_ref()
                            .or(self.oauth_existing.as_ref());
                        if oauth.is_some() {
                            edit::resolve_credential_env(self.provider, &self.credential, oauth)
                                .map(|_| ())
                        } else {
                            Ok(())
                        }
                    }
                    edit::CredentialKind::EnvVar => {
                        let oauth = self
                            .oauth_authorized
                            .as_ref()
                            .or(self.oauth_existing.as_ref());
                        edit::resolve_credential_env(self.provider, &self.credential, oauth)
                            .map(|_| ())
                    }
                }
            }
            Step::Params => {
                let visible = self.visible_fields();
                let Some(&i) = visible.get(self.field_idx) else {
                    return Ok(());
                };
                let field = &self.fields[i];
                // Core owns the required flag; we look up the descriptor
                // rather than trusting `field.required`, so a tampered
                // wizard state cannot bypass the policy.
                let descriptor = edit::descriptor(self.provider, field.key);
                if let FieldKind::Text(value) = &field.kind {
                    let value = value.trim();
                    if value.is_empty() {
                        if descriptor.map(|d| d.required).unwrap_or(false) {
                            return Err(format!(
                                "{} é obrigatório.",
                                descriptor.map(|d| d.label).unwrap_or(field.label)
                            ));
                        }
                        return Ok(()); // opcional em branco é removido só ao salvar
                    }
                    let mut known = BTreeMap::new();
                    for &j in &visible {
                        if let FieldKind::Text(v) = &self.fields[j].kind {
                            known.insert(self.fields[j].key.to_string(), v.trim().to_string());
                        }
                    }
                    return edit::validate_param(field.key, value, &known)
                        .map_err(|note| note.to_string());
                }
                Ok(())
            }
            Step::Interval => self.parse_interval().map(|_| ()),
            _ => Ok(()),
        };
        result.map_err(|note| note.to_string())
    }

    fn parse_interval(&self) -> Result<Option<u64>, cuotai_core::text::Localized> {
        // Delegates the parsing rules to the core account-edit module so the
        // bounds stay in sync with `Config::validate`. The wizard contributes
        // only the user-typed text. The TUI renders the Portuguese fallback
        // directly; the RF8 structured note (stable id + params) is unused
        // here but exposed by the core for the desktop catalog resolution.
        edit::parse_refresh_seconds(&self.interval)
    }

    fn env_status(&self) -> (&'static str, Color) {
        if !edit::valid_env_name(self.credential.trim()) {
            return ("NOME INVÁLIDO", DRACULA_RED);
        }
        // Mesma regra do backend: valor presente e não vazio após trim.
        match std::env::var(self.credential.trim()) {
            Ok(value) if !value.trim().is_empty() => ("PRESENTE", DRACULA_GREEN),
            _ => ("AUSENTE", DRACULA_YELLOW),
        }
    }

    fn build_config(&self) -> Result<Config, String> {
        // Translate the wizard's UI fields into the UI-neutral AccountDraft
        // and let the core account-edit module own the policy (name check,
        // OAuth preservation, managed-keys cleanup, native login params,
        // env-name/ARM validation, refresh bounds, insert into Config).
        // Only thing the wizard adds is what is purely UI: which fields are
        // currently visible (Choice/text resolution comes from the live
        // widgets).
        let visible = self.visible_fields();
        let mut draft_fields: Vec<edit::DraftField> = Vec::with_capacity(visible.len());
        for &i in &visible {
            let field = &self.fields[i];
            let value = match &field.kind {
                FieldKind::Text(value) => edit::DraftValue::Text(value.clone()),
                FieldKind::Choice { selected, .. } => edit::DraftValue::Choice(*selected),
            };
            draft_fields.push(edit::DraftField {
                key: field.key,
                value,
            });
        }
        let existing_account = self
            .editing
            .and_then(|i| self.base.accounts.get(i))
            .filter(|a| a.provider == self.provider);
        let existing_params = existing_account.map(|a| &a.params);
        let draft = edit::AccountDraft {
            provider: self.provider,
            name: &self.name,
            credential: &self.credential,
            oauth_mode: self.oauth_active(),
            oauth_existing: self.oauth_existing.as_ref(),
            oauth_authorized: self.oauth_authorized.clone(),
            oauth_login_params: self.oauth_login_params.clone(),
            existing_params,
            credential_params: self.credential_params.clone(),
            fields: &draft_fields,
            interval_text: &self.interval,
        };
        let account =
            edit::build_account(&draft, &self.base, self.editing).map_err(|e| e.to_string())?;
        self.base
            .upsert_account(account, self.editing)
            .map_err(|e| e.to_string())
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
        if let Some(advisory) = edit::cost_advisory(self.provider) {
            lines.push(Line::from(Span::styled(
                advisory.fallback,
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

        // RF4 conflict-recovery modal: layered on top of the wizard
        // body so the user keeps the visual context of the draft
        // they are about to (re)save. The wizard's key handler routes
        // `r` to `Action::ConflictReload` and `Esc` to
        // `Action::ConflictDismiss` while this overlay is showing.
        if let Some(message) = self.conflict.as_ref() {
            self.draw_conflict_modal(frame, area, message);
        }
    }

    /// Centered overlay shown when the save path returns
    /// `EditError::Conflict`. Renders above the wizard body so the
    /// user can read the typed message and pick reload/cancel.
    fn draw_conflict_modal(&self, frame: &mut Frame, area: Rect, message: &str) {
        let modal_w = area.width.saturating_sub(8).clamp(40, 78);
        let modal_h = 9u16;
        let modal = Rect {
            x: area.x + (area.width.saturating_sub(modal_w)) / 2,
            y: area.y + (area.height.saturating_sub(modal_h)) / 2,
            width: modal_w,
            height: modal_h,
        };
        frame.render_widget(Clear, modal);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(DRACULA_YELLOW))
            .title(Span::styled(
                " Arquivo mudou no disco ",
                Style::default()
                    .fg(DRACULA_YELLOW)
                    .add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(modal);
        frame.render_widget(block, modal);
        let lines = vec![
            Line::from(Span::styled(
                message.to_string(),
                Style::default().fg(DRACULA_FG),
            )),
            Line::default(),
            Line::from(Span::styled(
                "Recarregar substitui a versão em memória pelo conteúdo atual do",
                Style::default().fg(DRACULA_COMMENT),
            )),
            Line::from(Span::styled(
                "arquivo (sem alterar o rascunho do assistente). Cancelar mantém",
                Style::default().fg(DRACULA_COMMENT),
            )),
            Line::from(Span::styled(
                "a versão em memória para nova edição.",
                Style::default().fg(DRACULA_COMMENT),
            )),
            Line::default(),
            Line::from(Span::styled(
                "[r] recarregar do disco   [Esc] cancelar",
                Style::default()
                    .fg(DRACULA_CYAN)
                    .add_modifier(Modifier::BOLD),
            )),
        ];
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
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
                edit::oauth_registration_help(self.provider).fallback,
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
                edit::credential_help(self.provider).fallback,
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
            } else if let Some(reason) = edit::oauth_unsupported_reason(self.provider) {
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
                    "Intervalo individual desta conta em segundos ({}–{MAX_INTERVAL}); vazio herda o global:",
                    min_interval_for(self.provider),
                ),
                prompt,
            )),
            Line::default(),
            Line::from(Span::styled(format!("> {}", self.interval), accent)),
        ];
        if let Some(advisory) = edit::cost_advisory(self.provider) {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                advisory.fallback,
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

/// Opções não secretas do fluxo de login nativo, editadas no passo Credencial
/// ANTES de iniciar o login e gravadas em `OAuthConfig.options` (apenas
/// region/auth_method/start_url — nunca tokens). Pré-carregadas da config
/// existente ao editar uma conta já autenticada.
///
/// Schema é fornecido por [`edit::native_option_descriptors`]: o TUI só
/// mapeia cada descritor para o widget de Field, mantendo a fonte da verdade
/// no core.
fn native_option_fields(provider: Provider, existing: Option<&OAuthConfig>) -> Vec<Field> {
    edit::native_option_descriptors(provider)
        .iter()
        .map(|descriptor| descriptor_to_native_field(descriptor, existing))
        .collect()
}

fn descriptor_to_native_field(
    descriptor: &edit::NativeOptionDescriptor,
    existing: Option<&OAuthConfig>,
) -> Field {
    let mut field = Field::text(descriptor.key, descriptor.label, descriptor.help, false);
    let existing_value = existing.and_then(|o| o.options.get(descriptor.key));
    match descriptor.kind {
        edit::FieldDescriptorKind::Text => {
            if let Some(value) = existing_value {
                field.kind = FieldKind::Text(value.clone());
            }
        }
        edit::FieldDescriptorKind::Choice(choice_desc) => {
            let mut field = Field::choice(
                descriptor.key,
                descriptor.label,
                descriptor.help,
                choice_desc.options,
            );
            if let Some(value) = existing_value {
                if let Some(pos) = choice_desc
                    .options
                    .iter()
                    .position(|(option, _)| *option == value)
                {
                    if let FieldKind::Choice { selected, .. } = &mut field.kind {
                        *selected = pos;
                    }
                }
            }
            return field;
        }
    }
    field
}

/// Schema (key/label/help/required/options) é fornecido por
/// [`edit::field_descriptors`]; o TUI só materializa cada descritor num
/// widget de Field, pré-preenchendo valores da conta em edição.
fn default_fields(provider: Provider, existing: Option<&Account>) -> Vec<Field> {
    edit::field_descriptors(provider)
        .iter()
        .map(|descriptor| descriptor_to_field(descriptor, existing))
        .collect()
}

fn descriptor_to_field(descriptor: &edit::FieldDescriptor, existing: Option<&Account>) -> Field {
    let existing_value = existing.and_then(|a| a.params.get(descriptor.key));
    match descriptor.kind {
        edit::FieldDescriptorKind::Text => {
            let mut field = Field::text(
                descriptor.key,
                descriptor.label,
                descriptor.help,
                descriptor.required,
            );
            if let Some(value) = existing_value {
                field.kind = FieldKind::Text(value.clone());
            }
            field
        }
        edit::FieldDescriptorKind::Choice(choice_desc) => {
            let mut field = Field::choice(
                descriptor.key,
                descriptor.label,
                descriptor.help,
                choice_desc.options,
            );
            if let Some(value) = existing_value {
                if let Some(pos) = choice_desc
                    .options
                    .iter()
                    .position(|(option, _)| *option == value)
                {
                    if let FieldKind::Choice { selected, .. } = &mut field.kind {
                        *selected = pos;
                    }
                }
            }
            field
        }
    }
}

/// RF2.6: oferecer um atalho F2 só quando o core expõe o modo correspondente;
/// a TUI não decide o fluxo por `Provider`.
fn wizard_offers_mode(provider: Provider, mode_id: &str) -> bool {
    login::login_modes(provider)
        .modes
        .iter()
        .any(|mode: &LoginMode| mode.id == mode_id)
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

    #[test]
    fn wizard_visible_fields_match_core_visibility_decision() {
        // The wizard MUST NOT carry its own provider-specific visibility
        // rules. After the user flips the Grok product to "cli", the
        // session_env field becomes visible and team_id disappears — both
        // must agree with `edit::visible_field_indices`.
        let mut wiz = Wizard::new(&Config::default(), None);
        wiz.provider = Provider::Grok;
        wiz.fields = default_fields(Provider::Grok, None);
        let baseline = wiz.visible_fields();
        let product_index = wiz
            .fields
            .iter()
            .position(|f| f.key == "product")
            .expect("Grok descriptor exposes product");
        let session_index = wiz
            .fields
            .iter()
            .position(|f| f.key == "session_env")
            .expect("Grok descriptor exposes session_env");
        let team_index = wiz
            .fields
            .iter()
            .position(|f| f.key == "team_id")
            .expect("Grok descriptor exposes team_id");
        // Baseline (product=0, management mode): session_env hidden,
        // team_id visible — visibility decision lives in core.
        assert!(!baseline.contains(&session_index));
        assert!(baseline.contains(&team_index));
        // Cycle the product choice to "cli" (index 1) — visibility must
        // mirror core's decision (session_env becomes visible, team_id
        // disappears).
        if let FieldKind::Choice { selected, .. } = &mut wiz.fields[product_index].kind {
            *selected = 1;
        }
        let visible_after = wiz.visible_fields();
        assert!(visible_after.contains(&session_index));
        assert!(!visible_after.contains(&team_index));
    }

    #[test]
    fn conflict_modal_intercepts_keys_and_keeps_draft() {
        // RF4 contract: when `ConfigFile::save` returns
        // `EditError::Conflict`, the wizard must surface the conflict
        // modal without dropping the user's draft. The modal accepts
        // `r` to reload and `Esc` to dismiss; any other key is
        // ignored so the draft is not accidentally edited.
        let mut wiz = wizard_with("OpenAI", "OPENAI_ADMIN_KEY", "60");
        let draft_name = wiz.name.clone();
        let draft_interval = wiz.interval.clone();
        let draft_provider = wiz.provider;

        wiz.set_conflict("arquivo mudou".into());
        assert!(wiz.has_conflict());
        assert!(wiz.conflict().is_some());

        // Ignore a normal character while the modal is up.
        let ignored = wiz.handle(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(matches!(ignored, Action::Continue));
        // Draft fields are intact.
        assert_eq!(wiz.name, draft_name);
        assert_eq!(wiz.interval, draft_interval);
        assert_eq!(wiz.provider, draft_provider);

        // Esc dismisses the modal without changing the draft.
        let dismissed = wiz.handle(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(dismissed, Action::ConflictDismiss));
        wiz.clear_conflict();
        assert!(!wiz.has_conflict());

        // r triggers the reload action.
        wiz.set_conflict("arquivo mudou".into());
        let reload = wiz.handle(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert!(matches!(reload, Action::ConflictReload));
    }

    #[test]
    fn replace_base_swaps_schema_without_touching_draft() {
        // After a successful reload the wizard validates the same draft
        // against the freshly-loaded schema. `replace_base` MUST NOT
        // touch name/credential/fields/interval so the user can re-save
        // without re-typing.
        let mut wiz = wizard_with("OpenAI", "OPENAI_ADMIN_KEY", "60");
        let draft_name = wiz.name.clone();
        let draft_interval = wiz.interval.clone();

        let new_config = Config {
            accounts: vec![
                Account {
                    name: "Other".into(),
                    provider: Provider::AnthropicApi,
                    credential_env: Some("ANTHROPIC_ADMIN_KEY".into()),
                    oauth: None,
                    params: BTreeMap::new(),
                    refresh_seconds: Some(45),
                },
                Account {
                    name: "OpenAI".into(),
                    provider: Provider::OpenAiApi,
                    credential_env: Some("OPENAI_ADMIN_KEY".into()),
                    oauth: None,
                    params: BTreeMap::new(),
                    refresh_seconds: Some(60),
                },
            ],
            ..Config::default()
        };
        wiz.replace_base(&new_config);
        assert_eq!(wiz.name, draft_name);
        assert_eq!(wiz.interval, draft_interval);
        // The base now reflects the freshly-loaded config.
        assert_eq!(wiz.base.accounts.len(), 2);
    }

    #[test]
    fn wizard_prefills_aws_profile_from_stored_params() {
        // RF5: the wizard delegates the existing stored profile read to
        // core (`edit::stored_credential`) so a stored AWS CLI v2 profile
        // surfaces in the credential step instead of an empty slot.
        let config = Config {
            accounts: vec![Account {
                name: "Prod".into(),
                provider: Provider::AwsBedrock,
                credential_env: None,
                oauth: None,
                params: BTreeMap::from([("profile".into(), "default".into())]),
                refresh_seconds: None,
            }],
            ..Config::default()
        };
        let wiz = Wizard::new(&config, Some(0));
        assert_eq!(wiz.credential, "default");
        // AWS never exposes OAuth (per `credential_modes`).
        assert!(!wiz.oauth_active());
        assert!(!wiz.credential_supports_oauth());
        assert_eq!(
            wiz.validate_step(),
            Ok(()),
            "stored AWS profile must prefill and validate"
        );
    }

    #[test]
    fn wizard_prefills_token_plan_cli_profile_from_stored_params() {
        // RF5: AlibabaTokenPlan reads from params["cli_profile"] via core.
        // The credential slot is intentionally empty (the form does not
        // ask for an env-var name), and validation must accept it.
        let config = Config {
            accounts: vec![Account {
                name: "Qwen".into(),
                provider: Provider::AlibabaTokenPlan,
                credential_env: None,
                oauth: None,
                params: BTreeMap::from([("cli_profile".into(), "qwen-personal".into())]),
                refresh_seconds: None,
            }],
            ..Config::default()
        };
        let wiz = Wizard::new(&config, Some(0));
        assert_eq!(wiz.credential, "qwen-personal");
        assert_eq!(wiz.validate_step(), Ok(()));
    }

    #[test]
    fn wizard_tab_oauth_toggle_follows_credential_modes() {
        // RF5: Tab → OAuth only works for providers whose credential
        // modes expose oauth. AWS Bedrock and AlibabaTokenPlan must
        // refuse the toggle. Every other provider with native login
        // support must allow it.
        for provider in Provider::ALL {
            let config = Config {
                accounts: vec![Account {
                    name: "Test".into(),
                    provider,
                    credential_env: None,
                    oauth: None,
                    params: BTreeMap::new(),
                    refresh_seconds: None,
                }],
                ..Config::default()
            };
            let mut wiz = Wizard::new(&config, Some(0));
            wiz.provider = provider;
            wiz.step = Step::Credential;
            wiz.oauth_mode = false;
            let action = wiz.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
            // Tab toggles oauth_mode when the form supports it; otherwise
            // it is consumed by `handle` and the wizard stays in env mode.
            let expected = edit::credential_modes(provider).oauth;
            if expected {
                // Tab moves the wizard into OAuth mode (returns Continue).
                assert!(matches!(action, Action::Continue));
                assert!(wiz.oauth_mode, "{provider:?} should toggle to oauth");
            } else {
                // Tab is ignored for non-OAuth providers (returns Continue
                // without flipping the toggle).
                assert!(!wiz.oauth_mode, "{provider:?} must stay in env mode");
            }
        }
    }
}
