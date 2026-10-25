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
use cuotai_core::access::UsageAccess;
use cuotai_core::account_edit as edit;
use cuotai_core::login::{self, LoginCredentialStore, LoginMode, LoginOption, LoginRequest};
use cuotai_core::oauth::OAuthConfig;
use cuotai_core::user_session::{self, ReducedSession};
use cuotai_core::{Account, Config, Provider, Report, Status};
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
        /// PRD 0016 RF2: pasted sessions, already reduced by the core, to
        /// store in the vault with this save.
        sessions: Vec<ReducedSession>,
        /// SES-02: the core's clearance of the test before save; `Some`
        /// whenever `sessions` is not empty.
        clearance: Option<user_session::SaveClearance>,
    },
    /// Start any core-described flow through the same request/interface.
    Login {
        provider: Provider,
        request: Box<LoginRequest>,
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
    /// RF5.1: open the Models section of an already-configured account,
    /// chosen explicitly in the provider step (index into the base config).
    Models {
        index: usize,
    },
    /// PRD 0016: the user confirmed a vault change of the saved account's
    /// web session on the summary.
    SessionVault {
        index: usize,
        change: VaultChange,
    },
    /// PRD 0016 RF2.1: one read-only query of the draft with its pasted
    /// sessions; nothing is stored.
    TestSession {
        account: Box<Account>,
        sessions: Vec<ReducedSession>,
    },
}

/// PRD 0016: what a confirmed summary key changes in the vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VaultChange {
    /// RF1.2 (F3): move the `session_env`-style variables into the vault.
    Migrate,
    /// RF4.2 (F5): forget the session, deleting its vault entries.
    Forget,
}

const MAX_INTERVAL: u64 = edit::MAX_REFRESH_SECONDS;

/// Effective minimum interval accepted by the editor for the selected
/// provider. Metered overrides from 300 through 899 seconds remain valid
/// configuration and are raised to the 900-second runtime floor with a
/// report note, matching `Config::validate`.
fn min_interval_for(provider: Provider) -> u64 {
    if provider.is_metered() {
        Provider::METERED_MIN_CONFIGURED_REFRESH_SECONDS
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
    /// PRD 0016 RF2.1: only for providers whose core declares a session.
    Session,
    Interval,
    Summary,
}

/// PRD 0016 RF2.1: one session slot of the paste step. The pasted text is
/// never drawn and is overwritten whenever the step forgets it (clear,
/// provider switch, wizard closed).
struct SessionPaste {
    descriptor: &'static user_session::SessionDescriptor,
    text: String,
}

impl Drop for SessionPaste {
    fn drop(&mut self) {
        user_session::wipe(&mut self.text);
    }
}

impl SessionPaste {
    /// The core's reduction of the paste; `None` while nothing was pasted.
    fn reduce(&self, provider: Provider) -> Option<Result<ReducedSession, String>> {
        (!self.text.trim().is_empty()).then(|| {
            user_session::reduce(
                provider,
                self.descriptor.slot,
                &self.text,
                chrono::Utc::now(),
            )
            .map_err(|note| crate::ui::resolve_l10n(&note))
        })
    }
}

fn session_pastes(provider: Provider) -> Vec<SessionPaste> {
    user_session::descriptors(provider)
        .iter()
        .map(|descriptor| SessionPaste {
            descriptor,
            text: String::new(),
        })
        .collect()
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
        /// Auth-mode-filtered options. Each carries its full descriptor index
        /// so the draft remains unambiguous to `build_account`.
        options: Vec<edit::FieldOption>,
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
        options: Vec<edit::FieldOption>,
        selected: usize,
    ) -> Self {
        Self {
            key,
            label,
            help,
            required: false,
            kind: FieldKind::Choice { options, selected },
        }
    }
}
struct LoginField {
    option: LoginOption,
    registration: bool,
    kind: LoginFieldKind,
}

enum LoginFieldKind {
    Text(String),
    Choice { selected: usize },
}

impl LoginField {
    fn new(option: LoginOption, registration: bool, request: &LoginRequest) -> Self {
        let values = if registration {
            &request.registration
        } else {
            &request.options
        };
        let existing = values.get(&option.key);
        let kind = if let Some(choices) = &option.choices {
            let selected = existing
                .and_then(|value| choices.iter().position(|choice| choice.value == *value))
                .unwrap_or(0);
            LoginFieldKind::Choice { selected }
        } else {
            LoginFieldKind::Text(existing.cloned().unwrap_or_default())
        };
        Self {
            option,
            registration,
            kind,
        }
    }

    fn value(&self) -> String {
        match &self.kind {
            LoginFieldKind::Text(value) => value.trim().to_string(),
            LoginFieldKind::Choice { selected } => self
                .option
                .choices
                .as_ref()
                .and_then(|choices| choices.get(*selected))
                .map(|choice| choice.value.clone())
                .unwrap_or_default(),
        }
    }

    fn text_mut(&mut self) -> Option<&mut String> {
        match &mut self.kind {
            LoginFieldKind::Text(value) => Some(value),
            LoginFieldKind::Choice { .. } => None,
        }
    }
}

pub(crate) struct Wizard {
    base: Config,
    editing: Option<usize>,
    step: Step,
    provider: Provider,
    provider_sel: usize,
    /// Providers offered by the picker, from `edit::picker_providers`.
    picker: Vec<Provider>,
    provider_offset: u16,
    name: String,
    credential: String,
    fields: Vec<Field>,
    field_idx: usize,
    interval: String,
    summary_scroll: u16,
    error: Option<String>,
    conflict: Option<String>,
    notice: Option<String>,
    /// Core-owned discovery plus the selected credential/mode id.
    login: login::ProviderLogin,
    auth_mode: String,
    login_fields: Vec<LoginField>,
    login_idx: usize,
    /// Config OAuth da conta em edição, preservada enquanto a identidade não mudar.
    oauth_existing: Option<OAuthConfig>,
    /// Config autorizada por login concluído nesta sessão do assistente.
    oauth_authorized: Option<OAuthConfig>,
    /// Public params returned by an external CLI login (`Some` also means the
    /// flow completed; an empty map is a valid result for a future mode).
    external_login_params: Option<BTreeMap<String, String>>,
    /// Params públicos devolvidos pelo login concluído nesta sessão.
    oauth_login_params: BTreeMap<String, String>,
    /// Models picker of the provider step: highlighted position among the
    /// configured accounts of the highlighted provider.
    models_pick: Option<usize>,
    /// PRD 0005 RF3.4: set by the first Enter on the summary of an
    /// `Unavailable` account; the next Enter saves anyway, any other key
    /// clears it.
    confirm_save: bool,
    /// PRD 0016 RF1.2: set by the first F3 on the summary, which shows what
    /// the migration does; the next F3 confirms it, any other key clears it.
    confirm_migrate: bool,
    /// PRD 0016 RF4.2: the same two-step confirmation for F5, "forget this
    /// session".
    confirm_forget: bool,
    /// PRD 0016 RF2.1: the paste step's slots, the highlighted one and the
    /// last test's outcome (`true` when the query answered).
    sessions: Vec<SessionPaste>,
    session_idx: usize,
    session_test: Option<(bool, String)>,
    /// SES-02: the running test was started by Enter on the summary, so a
    /// cleared result saves. Any key withdraws it.
    test_then_save: bool,
    /// SES-02: the test before save could not reach the provider; the next
    /// Enter saves anyway with this report, any other key withdraws it.
    confirm_untested: Option<Box<Report>>,
}

impl Wizard {
    pub(crate) fn new(config: &Config, editing: Option<usize>) -> Self {
        let existing = editing.and_then(|i| config.accounts.get(i));
        let provider = existing.map(|a| a.provider).unwrap_or(Provider::OpenAiApi);
        // Core decides which providers the picker offers and their order.
        let picker = edit::picker_providers(existing.map(|a| a.provider));
        let provider_sel = picker.iter().position(|p| *p == provider).unwrap_or(0);
        let name = existing.map(|a| a.name.clone()).unwrap_or_default();
        let credential = match existing {
            Some(a) => edit::stored_credential(a),
            None => edit::suggested_credential(provider).to_string(),
        };
        let oauth_existing = existing
            .filter(|a| a.provider == provider)
            .and_then(|a| a.oauth.clone());
        let login = login::login_modes(provider);
        let auth_mode = initial_auth_mode(provider, &login, oauth_existing.is_some());
        let (fields, notices) = default_fields(
            provider,
            &auth_mode,
            existing.filter(|account| account.provider == provider),
        );
        let mode = login.modes.iter().find(|mode| mode.id == auth_mode);
        let login_fields = login_fields_for(provider, mode, existing, &fields);
        let notice = (!notices.is_empty()).then(|| {
            notices
                .iter()
                .map(|notice| notice.text())
                .collect::<Vec<_>>()
                .join("\n")
        });
        Self {
            base: config.clone(),
            editing,
            step: Step::Provider,
            provider,
            provider_sel,
            picker,
            provider_offset: 0,
            name,
            credential,
            fields,
            field_idx: 0,
            interval: existing
                .and_then(|a| a.refresh_seconds)
                .map(|seconds| seconds.to_string())
                .unwrap_or_default(),
            summary_scroll: 0,
            error: None,
            conflict: None,
            notice,
            login,
            auth_mode,
            login_fields,
            login_idx: 0,
            oauth_existing,
            oauth_authorized: None,
            external_login_params: None,
            oauth_login_params: BTreeMap::new(),
            models_pick: None,
            confirm_save: false,
            confirm_migrate: false,
            confirm_forget: false,
            sessions: session_pastes(provider),
            session_idx: 0,
            session_test: None,
            test_then_save: false,
            confirm_untested: None,
        }
    }

    /// SES-01: the editor of the account at `index` opened straight on its
    /// paste step ("Paste a new session" on the card or the details). Other
    /// providers open on the first step, as the editor does.
    pub(crate) fn paste_session(config: &Config, index: usize) -> Self {
        let mut wizard = Self::new(config, Some(index));
        if !wizard.sessions.is_empty() {
            wizard.step = Step::Session;
        }
        wizard
    }

    pub(crate) fn base_account(&self, index: usize) -> Option<&Account> {
        self.base.accounts.get(index)
    }

    /// Base-config indices of the accounts of the highlighted provider.
    fn provider_accounts(&self) -> Vec<usize> {
        let provider = self.provider_at_sel();
        (0..self.base.accounts.len())
            .filter(|&i| self.base.accounts[i].provider == provider)
            .collect()
    }

    /// Keys of the Models picker; `None` when closed.
    fn handle_models_pick(&mut self, code: KeyCode) -> Option<Action> {
        let pick = self.models_pick?;
        let accounts = self.provider_accounts();
        Some(match code {
            KeyCode::Esc => {
                self.models_pick = None;
                Action::Continue
            }
            KeyCode::Up => {
                self.models_pick = Some(pick.saturating_sub(1));
                Action::Continue
            }
            KeyCode::Down => {
                self.models_pick = Some((pick + 1).min(accounts.len().saturating_sub(1)));
                Action::Continue
            }
            KeyCode::Enter => match accounts.get(pick) {
                Some(&index) => {
                    self.models_pick = None;
                    Action::Models { index }
                }
                None => Action::Continue,
            },
            _ => Action::Continue,
        })
    }

    fn models_pick_lines(&self) -> Vec<Line<'static>> {
        let provider = self.provider_at_sel();
        let accounts = self.provider_accounts();
        let mut lines = vec![Line::from(Span::styled(
            format!("Modelos · {}", provider.label()),
            Style::default().add_modifier(Modifier::BOLD),
        ))];
        if accounts.is_empty() {
            // A new account has no saved credential: explain support only.
            let probe = Account {
                id: None,
                provider,
                ..Default::default()
            };
            let text = match cuotai_core::models::support(&probe) {
                Ok(source) => format!(
                    "Fonte: {}\nNenhuma conta deste provedor está configurada; grave a conta e abra Modelos para consultá-la.",
                    crate::ui::resolve_l10n(&source)
                ),
                Err(reason) => crate::ui::resolve_l10n(&reason),
            };
            lines.extend(text.lines().map(|l| Line::from(l.to_string())));
            return lines;
        }
        lines.push(Line::from(Span::styled(
            "Escolha a conta configurada a consultar (Enter abre Modelos):",
            Style::default().fg(DRACULA_COMMENT),
        )));
        let pick = self.models_pick.unwrap_or(0);
        for (pos, &i) in accounts.iter().enumerate() {
            let (marker, style) = if pos == pick {
                (
                    "▶ ",
                    Style::default()
                        .fg(DRACULA_CYAN)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ("  ", Style::default())
            };
            lines.push(Line::from(Span::styled(
                format!("{marker}{}", crate::ui::safe(&self.base.accounts[i].name)),
                style,
            )));
        }
        lines
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

    /// The saved account being edited, while the draft keeps its provider.
    fn saved_account(&self) -> Option<&Account> {
        self.editing
            .and_then(|i| self.base.accounts.get(i))
            .filter(|account| account.provider == self.provider)
    }

    /// PRD 0016 RF1: session slots the edited account keeps in the vault.
    fn stored_slots(&self) -> Vec<String> {
        self.saved_account()
            .and_then(|account| account.session.as_ref())
            .map(|session| session.slots.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// PRD 0016 RF1.2: sessions the saved account still reads from a
    /// variable and could move to the vault.
    fn migration_candidates(&self) -> Vec<user_session::MigrationCandidate> {
        self.saved_account()
            .map(user_session::migration_candidates)
            .unwrap_or_default()
    }

    /// After a migration was saved: the base is the new file and the draft
    /// no longer names the migrated variables, so saving the draft later
    /// does not bring them back.
    pub(crate) fn session_migrated(&mut self, config: &Config) {
        let migrated = self.migration_candidates();
        self.replace_base(config);
        for candidate in migrated {
            if let Some(Field {
                kind: FieldKind::Text(text),
                ..
            }) = self.fields.iter_mut().find(|f| f.key == candidate.slot)
            {
                text.clear();
            }
        }
        self.error = None;
        self.notice = Some(crate::ui::resolve_l10n(&user_session::migrated_notice()));
    }

    /// PRD 0016 RF4.2: the saved account keeps a session in the vault that
    /// F5 on the summary can forget.
    fn has_vault_session(&self) -> bool {
        self.saved_account()
            .is_some_and(|account| account.session.is_some())
    }

    /// After a forgotten session was saved: the base is the new file, so
    /// saving the draft later does not bring the reference back.
    pub(crate) fn session_forgotten(&mut self, config: &Config) {
        self.replace_base(config);
        self.error = None;
        self.notice = Some(crate::ui::resolve_l10n(&user_session::forgotten_notice()));
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
        self.picker[self.provider_sel]
    }

    /// Campos visíveis: decisão única no core via `edit::visible_field_indices`,
    /// que conhece o schema do provedor, o modo nativo, os params de login e as
    /// seleções de Choice atuais. O TUI apenas repete o resultado para indexar
    /// os widgets de Field.
    fn visible_fields(&self) -> Vec<usize> {
        let selections = self.choice_selections();
        let visibility = edit::FieldVisibility {
            auth_mode: &self.auth_mode,
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
            if let FieldKind::Choice { options, selected } = &field.kind {
                if let Some(option) = options.get(*selected) {
                    map.insert(field.key.to_string(), option.index);
                }
            }
        }
        map
    }

    fn current_text_mut(&mut self) -> Option<&mut String> {
        match self.step {
            Step::Name => Some(&mut self.name),
            Step::Credential => {
                if self.selected_mode().is_some() {
                    self.login_text_mut()
                } else {
                    Some(&mut self.credential)
                }
            }
            Step::Interval => Some(&mut self.interval),
            Step::Session => self
                .sessions
                .get_mut(self.session_idx)
                .map(|paste| &mut paste.text),
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
                    self.field_idx = 0;
                    self.credential = edit::suggested_credential(selected).to_string();
                    self.login = login::login_modes(selected);
                    self.auth_mode = initial_auth_mode(selected, &self.login, false);
                    let (fields, notices) = default_fields(selected, &self.auth_mode, None);
                    self.fields = fields;
                    self.notice = (!notices.is_empty()).then(|| {
                        notices
                            .iter()
                            .map(|notice| notice.text())
                            .collect::<Vec<_>>()
                            .join("\n")
                    });
                    self.reload_login_fields();
                    self.sessions = session_pastes(selected);
                    self.session_idx = 0;
                    self.session_test = None;
                    self.invalidate_oauth();
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
                } else if !self.sessions.is_empty() {
                    self.step = Step::Session;
                } else {
                    self.step = Step::Interval;
                }
            }
            Step::Session => self.step = Step::Interval,
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
            Step::Session => {
                let visible = self.visible_fields().len();
                self.field_idx = visible.saturating_sub(1);
                self.step = Step::Params;
            }
            Step::Interval if !self.sessions.is_empty() => self.step = Step::Session,
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
        let key = self.fields[idx].key;
        let changed = if let FieldKind::Choice { options, selected } = &mut self.fields[idx].kind {
            let len = options.len() as isize;
            *selected = ((*selected as isize + delta).rem_euclid(len)) as usize;
            options.get(*selected).map(|option| (key, option.value))
        } else {
            None
        };
        if let Some((key, value)) = changed {
            self.sync_field_choice_to_login(key, value);
        }
    }

    fn selected_mode(&self) -> Option<&LoginMode> {
        self.login
            .modes
            .iter()
            .find(|mode| mode.id == self.auth_mode)
    }

    fn auth_choice_count(&self) -> usize {
        usize::from(edit::credential_kind(self.provider) != edit::CredentialKind::None)
            + self
                .login
                .modes
                .iter()
                .filter(|mode| mode.unavailable_reason.is_none())
                .count()
    }

    fn select_auth_choice(&mut self, index: usize) {
        if index >= self.auth_choice_count() {
            return;
        }
        let has_credential = edit::credential_kind(self.provider) != edit::CredentialKind::None;
        let mode_index = index.saturating_sub(usize::from(has_credential));
        let next = if has_credential && index == 0 {
            "credential".to_string()
        } else {
            self.login
                .modes
                .iter()
                .filter(|mode| mode.unavailable_reason.is_none())
                .nth(mode_index)
                .map(|mode| mode.id.clone())
                .unwrap_or_else(|| "credential".to_string())
        };
        if next == self.auth_mode {
            return;
        }
        self.auth_mode = next;
        self.reconcile_field_choices();
        self.reload_login_fields();
        self.invalidate_oauth();
        self.error = None;
    }

    fn cycle_auth_mode(&mut self) {
        let count = self.auth_choice_count();
        if count < 2 {
            return;
        }
        let has_credential = edit::credential_kind(self.provider) != edit::CredentialKind::None;
        let current = if has_credential && self.auth_mode == "credential" {
            0
        } else {
            self.login
                .modes
                .iter()
                .filter(|mode| mode.unavailable_reason.is_none())
                .position(|mode| mode.id == self.auth_mode)
                .map(|index| index + usize::from(has_credential))
                .unwrap_or(0)
        };
        self.select_auth_choice((current + 1) % count);
    }

    fn reload_login_fields(&mut self) {
        let existing = self.editing.and_then(|index| self.base.accounts.get(index));
        self.login_fields =
            login_fields_for(self.provider, self.selected_mode(), existing, &self.fields);
        self.login_idx = 0;
    }

    fn reconcile_field_choices(&mut self) {
        let mut notices = Vec::new();
        for field in &mut self.fields {
            let FieldKind::Choice { options, selected } = &mut field.kind else {
                continue;
            };
            let current = options.get(*selected).map(|option| option.value);
            let Some(resolved) =
                edit::resolve_field_choice(self.provider, field.key, &self.auth_mode, current)
            else {
                continue;
            };
            let Some(mode_options) = edit::field_options(self.provider, field.key, &self.auth_mode)
            else {
                continue;
            };
            *selected = mode_options
                .options
                .iter()
                .position(|option| option.index == resolved.option.index)
                .unwrap_or(0);
            *options = mode_options.options;
            if let Some(notice) = resolved.notice {
                notices.push(notice);
            }
        }
        self.notice = (!notices.is_empty()).then(|| {
            notices
                .iter()
                .map(|notice| notice.text())
                .collect::<Vec<_>>()
                .join("\n")
        });
    }

    fn sync_field_choice_to_login(&mut self, key: &str, value: &str) {
        let Some(field) = self
            .login_fields
            .iter_mut()
            .find(|field| !field.registration && field.option.key == key)
        else {
            return;
        };
        let (Some(choices), LoginFieldKind::Choice { selected }) =
            (&field.option.choices, &mut field.kind)
        else {
            return;
        };
        if let Some(position) = choices.iter().position(|choice| choice.value == value) {
            *selected = position;
        }
    }

    fn sync_login_choice_to_field(&mut self, key: &str, value: &str) {
        let Some(field) = self.fields.iter_mut().find(|field| field.key == key) else {
            return;
        };
        let FieldKind::Choice { options, selected } = &mut field.kind else {
            return;
        };
        if let Some(position) = options.iter().position(|option| option.value == value) {
            *selected = position;
        }
    }

    fn oauth_active(&self) -> bool {
        self.selected_mode()
            .is_some_and(|mode| mode.credential_store == LoginCredentialStore::CuotaiVault)
    }

    fn native_login(&self) -> bool {
        self.selected_mode().is_some_and(|mode| {
            mode.credential_store == LoginCredentialStore::CuotaiVault
                && mode.registration.is_empty()
        })
    }

    fn login_rows(&self) -> usize {
        self.login_fields.len() + 1
    }

    fn login_text_mut(&mut self) -> Option<&mut String> {
        self.login_fields
            .get_mut(self.login_idx)
            .and_then(LoginField::text_mut)
    }

    fn cycle_login_option(&mut self, delta: isize) {
        let changed = {
            let Some(field) = self.login_fields.get_mut(self.login_idx) else {
                return;
            };
            let Some(choices) = &field.option.choices else {
                return;
            };
            let LoginFieldKind::Choice { selected } = &mut field.kind else {
                return;
            };
            let len = choices.len() as isize;
            *selected = ((*selected as isize + delta).rem_euclid(len)) as usize;
            choices
                .get(*selected)
                .map(|choice| (field.option.key.clone(), choice.value.clone()))
        };
        if let Some((key, value)) = changed {
            self.sync_login_choice_to_field(&key, &value);
        }
        self.invalidate_oauth();
    }

    /// Any identity/mode edit invalidates completed logins. An explicit F2
    /// reauth does NOT call this: it keeps the existing config and its id so
    /// cancellation leaves the old credential usable.
    fn invalidate_oauth(&mut self) {
        self.oauth_existing = None;
        self.oauth_authorized = None;
        self.external_login_params = None;
        self.oauth_login_params.clear();
    }

    fn login_values(&self) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
        let mut options = BTreeMap::new();
        let mut registration = BTreeMap::new();
        for field in &self.login_fields {
            let value = field.value();
            if value.is_empty() {
                continue;
            }
            if field.registration {
                registration.insert(field.option.key.clone(), value);
            } else {
                options.insert(field.option.key.clone(), value);
            }
        }
        (options, registration)
    }

    fn enter_login(&mut self) -> Action {
        if self.login_idx + 1 < self.login_rows() {
            if let Some(field) = self.login_fields.get(self.login_idx) {
                if let Err(error) = field.option.validate(&field.value()) {
                    self.error = Some(error.to_string());
                    return Action::Continue;
                }
            }
            self.error = None;
            self.login_idx += 1;
            return Action::Continue;
        }
        if !self.oauth_active() || self.oauth_authorized.is_some() || self.oauth_existing.is_some()
        {
            self.error = None;
            self.advance();
            return Action::Continue;
        }
        self.start_selected_login()
    }

    /// Emit the single generic login action. Core validates again at the
    /// boundary and owns the actual flow selection.
    fn start_selected_login(&mut self) -> Action {
        let Some(mode) = self.selected_mode().cloned() else {
            return Action::Continue;
        };
        for field in &self.login_fields {
            if let Err(error) = field.option.validate(&field.value()) {
                self.error = Some(error.to_string());
                return Action::Continue;
            }
        }
        let existing = self.editing.and_then(|index| self.base.accounts.get(index));
        let mut request = LoginRequest::for_account(self.provider, mode.id, existing);
        (request.options, request.registration) = self.login_values();
        self.error = None;
        Action::Login {
            provider: self.provider,
            request: Box::new(request),
        }
    }

    fn account_login_inputs(&self) -> Result<(String, Option<BTreeMap<String, String>>), String> {
        let Some(mode) = self.selected_mode() else {
            return Ok((self.credential.clone(), None));
        };
        let (options, _) = self.login_values();
        let empty = BTreeMap::new();
        let result = self.external_login_params.as_ref().unwrap_or(&empty);
        login::account_login_inputs(
            self.provider,
            &mode.id,
            &self.credential,
            &options,
            result,
            self.external_login_params.is_some(),
        )
        .map_err(|error| error.to_string())
    }

    pub(crate) fn login_finished(&mut self, config: OAuthConfig, params: BTreeMap<String, String>) {
        self.oauth_authorized = Some(config);
        self.external_login_params = None;
        for (key, value) in &params {
            if let Some(field) = self.fields.iter_mut().find(|f| f.key == key.as_str()) {
                match &mut field.kind {
                    FieldKind::Text(text) => *text = value.clone(),
                    FieldKind::Choice { options, selected } => {
                        if let Some(pos) = options.iter().position(|option| option.value == value) {
                            *selected = pos;
                        }
                    }
                }
            }
        }
        self.oauth_login_params = params;
        self.error = None;
        self.notice = Some(
            self.selected_mode()
                .map(|mode| mode.completion_notice.to_string())
                .unwrap_or_else(|| "Login autorizado.".into()),
        );
        if self.step == Step::Credential {
            self.advance();
        }
    }

    pub(crate) fn external_login_finished(&mut self, params: BTreeMap<String, String>) {
        self.external_login_params = Some(params);
        self.error = None;
        self.notice = Some(
            self.selected_mode()
                .map(|mode| mode.completion_notice.to_string())
                .unwrap_or_else(|| "Login concluído.".into()),
        );
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
        if let Some(action) = self.handle_models_pick(key.code) {
            return action;
        }
        if self.step == Step::Provider && key.code == KeyCode::Char('m') {
            self.models_pick = Some(0);
            return Action::Continue;
        }
        if key.code == KeyCode::Esc {
            return Action::Cancel;
        }
        // Aviso de sucesso é transitório: some na primeira tecla seguinte.
        self.notice = None;
        // RF3.4: any key other than the confirming Enter cancels "save anyway".
        let confirmed = std::mem::take(&mut self.confirm_save);
        // PRD 0016 RF1.2: F3 on the summary first shows what the migration
        // does; only a second F3 moves the session. Any other key cancels.
        let migrate_confirmed = std::mem::take(&mut self.confirm_migrate);
        let forget_confirmed = std::mem::take(&mut self.confirm_forget);
        // SES-02: any key withdraws a pending save after the test, and only
        // the next Enter confirms "Save anyway".
        self.test_then_save = false;
        let untested_confirmed = self.confirm_untested.take();
        if self.step == Step::Summary && key.code == KeyCode::F(3) {
            let Some(index) = self
                .editing
                .filter(|_| !self.migration_candidates().is_empty())
            else {
                return Action::Continue;
            };
            if migrate_confirmed {
                return Action::SessionVault {
                    index,
                    change: VaultChange::Migrate,
                };
            }
            self.error = None;
            self.confirm_migrate = true;
            return Action::Continue;
        }
        // PRD 0016 RF4.2: F5 on the summary asks first; a second F5 forgets
        // the vault session. Any other key cancels.
        if self.step == Step::Summary && key.code == KeyCode::F(5) {
            let Some(index) = self.editing.filter(|_| self.has_vault_session()) else {
                return Action::Continue;
            };
            if forget_confirmed {
                return Action::SessionVault {
                    index,
                    change: VaultChange::Forget,
                };
            }
            self.error = None;
            self.confirm_forget = true;
            return Action::Continue;
        }
        // PRD 0016 RF2.1: F4 tests the pasted sessions with one read-only
        // query; ↑↓ pick the slot.
        if self.step == Step::Session {
            match key.code {
                KeyCode::F(4) => return self.test_session(),
                KeyCode::Up => {
                    self.session_idx = self.session_idx.saturating_sub(1);
                    return Action::Continue;
                }
                KeyCode::Down => {
                    self.session_idx = (self.session_idx + 1).min(self.sessions.len() - 1);
                    return Action::Continue;
                }
                _ => {}
            }
        }
        // Any selected core mode uses the same field navigation and F2 action.
        if self.step == Step::Credential && self.selected_mode().is_some() {
            match key.code {
                KeyCode::Enter => return self.enter_login(),
                KeyCode::Tab => {
                    self.cycle_auth_mode();
                    return Action::Continue;
                }
                KeyCode::F(2) => {
                    // Explicit reauth keeps oauth_existing (and therefore its
                    // credential_id) so cancel/failure leaves the old login usable.
                    self.oauth_authorized = None;
                    self.external_login_params = None;
                    self.oauth_login_params.clear();
                    return self.start_selected_login();
                }
                KeyCode::Up => {
                    self.login_idx = self.login_idx.saturating_sub(1);
                    return Action::Continue;
                }
                KeyCode::Down => {
                    self.login_idx = (self.login_idx + 1).min(self.login_rows() - 1);
                    return Action::Continue;
                }
                KeyCode::Left => {
                    self.cycle_login_option(-1);
                    return Action::Continue;
                }
                KeyCode::Right => {
                    self.cycle_login_option(1);
                    return Action::Continue;
                }
                KeyCode::BackTab => {
                    self.error = None;
                    if self.login_idx > 0 {
                        self.login_idx -= 1;
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
                        Ok(_) if !confirmed && self.access().save_confirmation().is_some() => {
                            self.error = None;
                            self.confirm_save = true;
                            Action::Continue
                        }
                        Ok(config) => self.save(config, untested_confirmed),
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
            (KeyCode::Tab, _) if self.step == Step::Credential && self.auth_choice_count() > 1 => {
                self.cycle_auth_mode();
            }
            (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => {
                if let Some(buf) = self.current_text_mut() {
                    user_session::wipe(buf);
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
                    self.provider_sel = (self.provider_sel + 1).min(self.picker.len() - 1);
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
                } else if matches!(
                    self.step,
                    Step::Name | Step::Credential | Step::Params | Step::Session
                ) && !c.is_control()
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
            match self.step {
                Step::Summary if self.confirm_save => "Gravar mesmo assim",
                Step::Summary => "Gravar",
                _ => "Próximo",
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
            // The picker is keyboard-only (wheel moves it via `handle`).
            MouseEventKind::Down(MouseButton::Left) if self.models_pick.is_some() => {
                return Action::Continue;
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
                    < self
                        .picker
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
            // The same logical rows are produced by draw_credential below:
            // mode list, help, then each descriptor and the confirmation row.
            Step::Credential => {
                let choices = self.auth_choice_count() as u16;
                if row >= 1 && row <= choices {
                    self.select_auth_choice(usize::from(row - 1));
                    return Action::Continue;
                }
                if self.selected_mode().is_none() {
                    return Action::Continue;
                }
                let mut line = choices + 4;
                for (index, field) in self.login_fields.iter().enumerate() {
                    let block = 1 + u16::from(index == self.login_idx);
                    if row >= line && row < line + block {
                        if index != self.login_idx {
                            self.login_idx = index;
                        } else if row == line && field.option.choices.is_some() {
                            return self.handle(right());
                        }
                        return Action::Continue;
                    }
                    line += block;
                }
                if row == line {
                    if self.login_idx == self.login_fields.len() {
                        return self.handle(enter());
                    }
                    self.login_idx = self.login_fields.len();
                }
            }
            // Name/Interval/Credential em modo variável de ambiente: um único
            // campo de texto já focado; Summary: só rolagem (tratada acima).
            _ => {}
        }
        Action::Continue
    }

    /// Editing any login input invalidates its previous result. Reauth via F2
    /// deliberately does not edit an input and keeps the existing identity.
    fn after_credential_edit(&mut self) {
        if self.step == Step::Credential
            && self.selected_mode().is_some()
            && self.login_idx < self.login_fields.len()
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
                let (credential, _) = self.account_login_inputs()?;
                match edit::credential_kind(self.provider) {
                    edit::CredentialKind::AwsProfile => {
                        edit::apply_aws_profile(self.provider, &credential)
                    }
                    edit::CredentialKind::None => {
                        let oauth = self
                            .oauth_authorized
                            .as_ref()
                            .or(self.oauth_existing.as_ref());
                        if oauth.is_some() {
                            edit::resolve_credential_env(self.provider, &credential, oauth)
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
                        edit::resolve_credential_env(self.provider, &credential, oauth).map(|_| ())
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
            // PRD 0016 RF2.2: a paste the core cannot reduce stops here,
            // with the cookie it is about and never a value.
            Step::Session => {
                if let Some(Err(message)) = self
                    .sessions
                    .iter()
                    .find_map(|p| p.reduce(self.provider).filter(Result::is_err))
                {
                    return Err(message);
                }
                Ok(())
            }
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

    /// The current form values in the core draft shape (visible fields only).
    fn draft_fields(&self) -> Vec<edit::DraftField> {
        self.visible_fields()
            .into_iter()
            .map(|i| {
                let field = &self.fields[i];
                let value = match &field.kind {
                    FieldKind::Text(value) => edit::DraftValue::Text(value.clone()),
                    FieldKind::Choice { options, selected } => {
                        edit::DraftValue::Choice(options[*selected].index)
                    }
                };
                edit::DraftField {
                    key: field.key,
                    value,
                }
            })
            .collect()
    }

    /// PRD 0005 RF3: how the draft's usage will be read, from the core.
    fn access(&self) -> UsageAccess {
        // A pasted session counts as stored: it is written with the save.
        let mut slots = self.stored_slots();
        slots.extend(
            self.reduced_sessions()
                .iter()
                .map(|session| session.slot().to_string()),
        );
        edit::draft_access(self.provider, &self.auth_mode, &self.draft_fields(), &slots)
    }

    /// PRD 0016 RF2: the pastes the core could reduce, ready for the vault.
    fn reduced_sessions(&self) -> Vec<ReducedSession> {
        self.sessions
            .iter()
            .filter_map(|paste| paste.reduce(self.provider)?.ok())
            .collect()
    }

    /// The save of `config`. SES-02: pasted sessions go through the core's
    /// test before save first; `untested` is the unreachable test the user
    /// now confirmed to save anyway.
    fn save(&mut self, config: Config, untested: Option<Box<Report>>) -> Action {
        let sessions = self.reduced_sessions();
        if sessions.is_empty() {
            return Action::Save {
                config,
                edit: self.editing,
                sessions,
                clearance: None,
            };
        }
        if let Some(report) = untested {
            return match user_session::clear_for_save(&report, true) {
                Ok(clearance) => Action::Save {
                    config,
                    edit: self.editing,
                    sessions,
                    clearance: Some(clearance),
                },
                Err(refused) => {
                    self.error = Some(crate::ui::resolve_l10n(&refused.note));
                    Action::Continue
                }
            };
        }
        let index = self.editing.unwrap_or(config.accounts.len() - 1);
        self.error = None;
        self.test_then_save = true;
        self.session_test = None;
        self.notice = Some("Testando a sessão antes de salvar…".into());
        Action::TestSession {
            account: Box::new(config.accounts[index].clone()),
            sessions,
        }
    }

    /// PRD 0016 RF2.1: F4 on the paste step. The draft must be complete and
    /// at least one paste reducible; the query itself runs in `ui.rs`.
    fn test_session(&mut self) -> Action {
        self.session_test = None;
        if let Err(message) = self.validate_step() {
            self.error = Some(message);
            return Action::Continue;
        }
        let sessions = self.reduced_sessions();
        if sessions.is_empty() {
            self.error = Some("Cole uma sessão antes de testar.".into());
            return Action::Continue;
        }
        match self.build_config() {
            Ok(config) => {
                let index = self.editing.unwrap_or(config.accounts.len() - 1);
                self.error = None;
                self.session_test = Some((true, "Testando a sessão…".into()));
                Action::TestSession {
                    account: Box::new(config.accounts[index].clone()),
                    sessions,
                }
            }
            Err(message) => {
                self.error = Some(message);
                Action::Continue
            }
        }
    }

    /// The outcome of [`Action::TestSession`]: whether the read-only query
    /// answered, with the core's note or the session's expiry. SES-02: after
    /// a test started by the save, the core's clearance saves (returned
    /// action); a transport failure offers "Save anyway" on the next Enter
    /// and any other failure blocks the save.
    pub(crate) fn session_tested(&mut self, report: &Report) -> Option<Action> {
        let resolve = crate::ui::resolve_l10n;
        let text = if report.status == Status::Ready {
            let mut text = format!(
                "Sessão aceita: a consulta de leitura respondeu ({} métricas).",
                report.metrics.len()
            );
            if let Some(expiry) = user_session::report_expiry(report, chrono::Utc::now()) {
                text = format!("{text} {}", resolve(&expiry.label));
            }
            text
        } else {
            let status = resolve(&cuotai_core::text::status_l10n(report.status));
            match report.notes_l10n.first() {
                Some(note) => format!("{status}: {}", resolve(note)),
                None => status,
            }
        };
        self.session_test = Some((report.status == Status::Ready, text));
        if !std::mem::take(&mut self.test_then_save) {
            return None;
        }
        self.notice = None;
        match user_session::clear_for_save(report, false) {
            Ok(clearance) => match self.build_config() {
                Ok(config) => Some(Action::Save {
                    config,
                    edit: self.editing,
                    sessions: self.reduced_sessions(),
                    clearance: Some(clearance),
                }),
                Err(message) => {
                    self.error = Some(message);
                    None
                }
            },
            Err(refused) => {
                let mut message = resolve(&refused.note);
                if refused.save_anyway {
                    message.push_str(" Enter: salvar mesmo assim.");
                    self.confirm_untested = Some(Box::new(report.clone()));
                }
                self.error = Some(message);
                None
            }
        }
    }

    /// PRD 0016 RF2.1: bracketed paste is on only while this step shows,
    /// so a pasted table or `cURL` arrives whole instead of as keystrokes.
    pub(crate) fn accepts_paste(&self) -> bool {
        self.step == Step::Session && self.conflict.is_none()
    }

    /// A bracketed paste into the highlighted session slot.
    pub(crate) fn paste(&mut self, text: &str) {
        if !self.accepts_paste() {
            return;
        }
        if let Some(paste) = self.sessions.get_mut(self.session_idx) {
            paste.text.push_str(text);
            self.error = None;
            self.session_test = None;
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
        let draft_fields = self.draft_fields();
        let existing_account = self
            .editing
            .and_then(|i| self.base.accounts.get(i))
            .filter(|a| a.provider == self.provider);
        let existing_params = existing_account.map(|a| &a.params);
        let (credential, credential_params) = self.account_login_inputs()?;
        let draft = edit::AccountDraft {
            provider: self.provider,
            name: &self.name,
            credential: &credential,
            auth_mode: &self.auth_mode,
            oauth_mode: self.oauth_active(),
            oauth_existing: self.oauth_existing.as_ref(),
            oauth_authorized: self.oauth_authorized.clone(),
            oauth_login_params: self.oauth_login_params.clone(),
            existing_params,
            credential_params,
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
        if let Some(mode) = self.selected_mode() {
            let (status, color) =
                if self.oauth_authorized.is_some() || self.external_login_params.is_some() {
                    ("LOGIN AUTORIZADO", DRACULA_GREEN)
                } else if self.oauth_existing.is_some() {
                    ("LOGIN EXISTENTE", DRACULA_GREEN)
                } else {
                    ("LOGIN PENDENTE", DRACULA_YELLOW)
                };
            lines.push(Line::from(vec![
                Span::styled("Autenticação: ", dim),
                Span::raw(mode.label.to_string()),
                Span::raw("  "),
                Span::styled(status, Style::default().fg(color)),
            ]));
            for field in &self.login_fields {
                lines.push(Line::from(format!(
                    "  {} = {}",
                    field.option.key,
                    crate::ui::safe(&field.value())
                )));
            }
            lines.push(Line::from(Span::styled(
                if mode.credential_store == LoginCredentialStore::CuotaiVault {
                    "(o token fica no armazenamento seguro; nunca é exibido nem gravado no TOML)"
                } else {
                    "(a credencial fica no armazenamento do CLI externo, fora do cofre Cuotai)"
                },
                dim,
            )));
        } else {
            lines.push(Line::from(vec![
                Span::styled("Credencial: ", dim),
                Span::raw(crate::ui::safe(self.credential.trim()).into_owned()),
            ]));
            if edit::credential_kind(self.provider) == edit::CredentialKind::EnvVar {
                let (status, color) = self.env_status();
                lines.push(Line::from(vec![
                    Span::styled("Status no ambiente: ", dim),
                    Span::styled(status, Style::default().fg(color)),
                ]));
            }
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
        // PRD 0016 RF1: sessions kept in the vault and, for sessions still
        // read from a variable, the explicit two-step move to the vault.
        if let Some(session) = self.saved_account().and_then(|a| a.session.as_ref()) {
            for slot in &session.slots {
                if let Some(descriptor) = user_session::descriptor(self.provider, slot) {
                    lines.push(Line::from(vec![
                        Span::styled("Sessão no cofre do sistema: ", dim),
                        Span::raw(crate::ui::resolve_l10n(&descriptor.label())),
                    ]));
                }
            }
            // PRD 0016 RF4.2: forgetting deletes the vault entries.
            lines.push(Line::from(Span::styled(
                if self.confirm_forget {
                    "F5 de novo apaga a sessão do cofre do sistema; qualquer outra tecla cancela."
                } else {
                    "F5 esquece esta sessão: apaga a entrada do cofre do sistema (pede confirmação)."
                },
                Style::default().fg(DRACULA_YELLOW),
            )));
        }
        let candidates = self.migration_candidates();
        if !candidates.is_empty() {
            let warn = Style::default().fg(DRACULA_YELLOW);
            for candidate in &candidates {
                lines.push(Line::from(Span::styled(
                    crate::ui::resolve_l10n(&candidate.consent),
                    warn,
                )));
            }
            lines.push(Line::from(Span::styled(
                if self.confirm_migrate {
                    "F3 de novo confirma a migração para o cofre; qualquer outra tecla cancela."
                } else {
                    "F3 migra a sessão para o cofre do sistema (pede confirmação)."
                },
                warn,
            )));
        }
        // PRD 0016 RF2: what the save writes to the vault (names only).
        for paste in &self.sessions {
            if let Some(Ok(reduced)) = paste.reduce(self.provider) {
                lines.push(Line::from(vec![
                    Span::styled("Sessão colada (vai para o cofre ao gravar): ", dim),
                    Span::raw(crate::ui::resolve_l10n(&paste.descriptor.label())),
                    Span::styled(" — ", dim),
                    Span::raw(crate::ui::resolve_l10n(
                        &reduced.summary(chrono::Utc::now()),
                    )),
                ]));
            }
        }
        // PRD 0005 RF3.3/RF3.4: cost and "no query" warnings come from the
        // same core descriptor; an Unavailable account shows its reason here,
        // before the save that needs a second, explicit Enter.
        let access = self.access();
        for warning in access
            .cost_advisory()
            .into_iter()
            .chain(access.save_confirmation())
        {
            lines.push(Line::from(Span::styled(
                crate::ui::resolve_l10n(&warning),
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

        let has_session = !self.sessions.is_empty();
        let total = if has_session { 7 } else { 6 };
        let (position, label) = match self.step {
            Step::Provider => (1, "Provedor"),
            Step::Name => (2, "Nome da conta"),
            Step::Credential => (3, "Credencial"),
            Step::Params => (4, "Parâmetros"),
            Step::Session => (5, "Colar sessão"),
            Step::Interval => (5 + usize::from(has_session), "Intervalo"),
            Step::Summary => (total, "Revisão"),
        };
        let title = format!("Assistente — {position}/{total} {label}");
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
            Step::Provider if self.models_pick.is_some() => frame.render_widget(
                Paragraph::new(self.models_pick_lines()).wrap(Wrap { trim: false }),
                inner,
            ),
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
            Step::Session => self.draw_session(frame, inner),
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
        } else if self.confirm_save {
            // The reason itself is already in the summary body (RF3.4).
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "Esta conta não tem consulta de uso. Enter grava mesmo assim; outra tecla cancela.",
                    Style::default().fg(DRACULA_YELLOW),
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
            Step::Provider if self.models_pick.is_some() => {
                " ↑↓ conta  Enter abrir Modelos  Esc voltar aos provedores"
            }
            Step::Provider => " ↑↓ navegar  Enter selecionar  m Modelos  Esc cancelar",
            Step::Params => {
                " Enter próximo  Shift+Tab voltar  ↑↓/←→ opções  Ctrl+U limpar  Esc cancelar"
            }
            Step::Summary
                if !self.migration_candidates().is_empty() && self.has_vault_session() =>
            {
                " Enter GRAVAR  F3 sessão → cofre  F5 esquecer sessão  ↑↓ rolar  Shift+Tab voltar  Esc cancelar"
            }
            Step::Summary if !self.migration_candidates().is_empty() => {
                " Enter GRAVAR  F3 sessão → cofre  ↑↓ rolar  Shift+Tab voltar  Esc cancelar sem gravar"
            }
            Step::Summary if self.has_vault_session() => {
                " Enter GRAVAR  F5 esquecer sessão  ↑↓ rolar  Shift+Tab voltar  Esc cancelar sem gravar"
            }
            Step::Summary => {
                " Enter GRAVAR no arquivo  ↑↓ rolar  Shift+Tab voltar  Esc cancelar sem gravar"
            }
            Step::Credential => {
                if self.selected_mode().is_some() {
                    " Enter próximo/avançar  F2 (re)login  ←→ opções  Tab alterna modo  ↑↓ campos  Shift+Tab voltar  Esc cancela"
                } else if self.auth_choice_count() > 1 {
                    " Enter próximo  Tab alterna modos de login  Shift+Tab voltar  Ctrl+U limpar  Esc cancelar"
                } else {
                    " Enter próximo  Shift+Tab voltar  Ctrl+U limpar  Esc cancelar"
                }
            }
            Step::Session => {
                " ↑↓ sessão  cole com o atalho do terminal  F4 testar  Ctrl+U limpar  Enter próximo  Shift+Tab voltar  Esc cancelar"
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
        let total = self.picker.len();
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
        for (i, provider) in self
            .picker
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
            // Clean names only: how each integration works lives in the docs.
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
        lines.push(Line::from(
            "Modo de autenticação (Tab alterna; Enter/F2 confirma):",
        ));
        let has_credential = edit::credential_kind(self.provider) != edit::CredentialKind::None;
        if has_credential {
            let current = self.auth_mode == "credential";
            lines.push(Line::from(format!(
                "{} Credencial manual (perfil/variável conforme o provedor)",
                if current { "▶" } else { " " }
            )));
        }
        for mode in self
            .login
            .modes
            .iter()
            .filter(|mode| mode.unavailable_reason.is_none())
        {
            lines.push(Line::from(format!(
                "{} {}",
                if mode.id == self.auth_mode {
                    "▶"
                } else {
                    " "
                },
                mode.label
            )));
        }
        lines.push(Line::default());
        if let Some(mode) = self.selected_mode() {
            lines.push(Line::from(Span::styled(
                mode.help.to_string(),
                Style::default().fg(DRACULA_COMMENT),
            )));
            lines.push(Line::default());
            for (index, field) in self.login_fields.iter().enumerate() {
                let current = self.login_idx == index;
                let style = if current {
                    Style::default()
                        .fg(DRACULA_CYAN)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let value = match &field.kind {
                    LoginFieldKind::Text(value) => crate::ui::safe(value).into_owned(),
                    LoginFieldKind::Choice { selected } => field
                        .option
                        .choices
                        .as_ref()
                        .and_then(|choices| choices.get(*selected))
                        .map(|choice| choice.label.to_string())
                        .unwrap_or_default(),
                };
                lines.push(Line::from(Span::styled(
                    format!(
                        "{}{}: {}",
                        if current { "▶ " } else { "  " },
                        field.option.label,
                        value
                    ),
                    style,
                )));
                if current {
                    lines.push(Line::from(Span::styled(
                        format!("    {}", field.option.help),
                        Style::default().fg(DRACULA_COMMENT),
                    )));
                }
            }
            let current = self.login_idx == self.login_fields.len();
            let action = if self.oauth_authorized.is_some() {
                "Login autorizado — Enter continua; F2 refaz reutilizando a identidade"
            } else if self.oauth_existing.is_some() {
                "Login existente — Enter continua; F2 refaz reutilizando a identidade"
            } else if self.external_login_params.is_some() {
                "Login concluído — Enter continua; F2 refaz"
            } else if mode.credential_store == LoginCredentialStore::CuotaiVault {
                "Enter inicia o login; F2 também inicia"
            } else {
                "Enter avança sem novo login; F2 inicia o login do CLI"
            };
            lines.push(Line::from(Span::styled(
                format!("{}{}", if current { "▶ " } else { "  " }, action),
                if current {
                    Style::default()
                        .fg(DRACULA_CYAN)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            )));
        } else {
            match edit::credential_kind(self.provider) {
                edit::CredentialKind::EnvVar => {
                    lines.push(Line::from(
                        "NOME da variável de ambiente exportada com a credencial (nunca o valor):",
                    ));
                    lines.push(Line::from(format!(
                        "> {}",
                        crate::ui::safe(&self.credential)
                    )));
                    let (status, color) = self.env_status();
                    lines.push(Line::from(vec![
                        Span::raw("Status no ambiente: "),
                        Span::styled(status, Style::default().fg(color)),
                    ]));
                }
                edit::CredentialKind::AwsProfile => {
                    lines.push(Line::from(
                        "Perfil do AWS CLI v2 usado para as credenciais:",
                    ));
                    lines.push(Line::from(format!(
                        "> {}",
                        crate::ui::safe(&self.credential)
                    )));
                }
                edit::CredentialKind::None => {}
            }
            let help = crate::ui::resolve_l10n(&edit::credential_help(self.provider));
            if !help.is_empty() {
                lines.push(Line::from(Span::styled(
                    help,
                    Style::default().fg(DRACULA_COMMENT),
                )));
            }
            if let Some(reason) = &self.login.unsupported_reason {
                lines.push(Line::from(Span::styled(
                    format!("Login integrado indisponível: {reason}"),
                    Style::default().fg(DRACULA_COMMENT),
                )));
            }
        }
        // PRD 0005 RF3.2: the access notes share the credential help area,
        // below every clickable row so the mouse hit-test is unchanged.
        let access = self.access();
        for note in access.notes() {
            let warn = access.reason.as_ref() == Some(&note);
            lines.push(Line::from(Span::styled(
                crate::ui::resolve_l10n(&note),
                Style::default().fg(if warn {
                    DRACULA_YELLOW
                } else {
                    DRACULA_COMMENT
                }),
            )));
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
                FieldKind::Choice { options, selected } => {
                    format!("< {} >", options[*selected].label)
                }
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

    /// PRD 0016 RF2.1: per slot, what it is, where to copy it from, the
    /// only hosts that receive it and the masked paste with the core's
    /// reduction (names and counts, never a value); then the last test.
    fn draw_session(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let resolve = crate::ui::resolve_l10n;
        let dim = Style::default().fg(DRACULA_COMMENT);
        let now = chrono::Utc::now();
        let stored = self.stored_slots();
        let mut lines = vec![
            Line::from(Span::styled(
                "Colar sessão (opcional): Cuotai guarda só os cookies desta sessão no cofre do sistema; o resto do texto colado é descartado.",
                Style::default().fg(DRACULA_CYAN),
            )),
        ];
        // PRD 0016 RF4.1: the `UserSession` level and the risk warning.
        if let Some(notice) = user_session::paste_notice(self.provider) {
            lines.push(Line::from(vec![
                Span::styled(
                    resolve(&notice.level),
                    Style::default()
                        .fg(DRACULA_YELLOW)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" · ", dim),
                Span::styled(
                    resolve(&notice.warning),
                    Style::default().fg(DRACULA_YELLOW),
                ),
            ]));
        }
        lines.push(Line::default());
        for (index, paste) in self.sessions.iter().enumerate() {
            let focused = index == self.session_idx;
            let marker = if focused { "▶ " } else { "  " };
            lines.push(Line::from(Span::styled(
                format!("{marker}{}", resolve(&paste.descriptor.label())),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            if focused {
                lines.push(Line::from(Span::styled(
                    resolve(&paste.descriptor.instructions()),
                    dim,
                )));
                lines.push(Line::from(Span::styled(
                    format!("Enviada só a: {}", paste.descriptor.hosts.join(", ")),
                    dim,
                )));
            }
            let chars = paste.text.chars().count();
            let field = if chars == 0 && stored.iter().any(|s| s == paste.descriptor.slot) {
                "> (sessão no cofre; colar outra a substitui)".to_string()
            } else if chars == 0 {
                "> (vazio)".to_string()
            } else {
                format!(
                    "> {} ({chars} caracteres colados)",
                    "•".repeat(chars.min(24))
                )
            };
            lines.push(Line::from(Span::styled(
                field,
                Style::default().fg(DRACULA_GREEN),
            )));
            match paste.reduce(self.provider) {
                Some(Ok(reduced)) => lines.push(Line::from(Span::styled(
                    resolve(&reduced.summary(now)),
                    Style::default().fg(DRACULA_GREEN),
                ))),
                Some(Err(message)) => lines.push(Line::from(Span::styled(
                    message,
                    Style::default().fg(DRACULA_RED),
                ))),
                None => {}
            }
            lines.push(Line::default());
        }
        if let Some((ok, text)) = &self.session_test {
            lines.push(Line::from(Span::styled(
                text.clone(),
                Style::default().fg(if *ok { DRACULA_GREEN } else { DRACULA_YELLOW }),
            )));
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
        if let Some(advisory) = self.access().cost_advisory() {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                crate::ui::resolve_l10n(&advisory),
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

fn initial_auth_mode(provider: Provider, login: &login::ProviderLogin, has_oauth: bool) -> String {
    if has_oauth
        && login.modes.iter().any(|mode| {
            mode.id == login::MODE_OAUTH
                && mode.credential_store == LoginCredentialStore::CuotaiVault
        })
    {
        return login::MODE_OAUTH.to_string();
    }
    if edit::credential_kind(provider) != edit::CredentialKind::None {
        return "credential".to_string();
    }
    login
        .modes
        .iter()
        .find(|mode| mode.unavailable_reason.is_none())
        .map(|mode| mode.id.clone())
        .unwrap_or_else(|| "credential".to_string())
}

fn login_fields_for(
    provider: Provider,
    mode: Option<&LoginMode>,
    existing: Option<&Account>,
    fields: &[Field],
) -> Vec<LoginField> {
    let Some(mode) = mode else {
        return Vec::new();
    };
    let mut request = LoginRequest::for_account(provider, &mode.id, existing);
    for field in fields {
        let FieldKind::Choice { options, selected } = &field.kind else {
            continue;
        };
        if !mode.options.iter().any(|option| option.key == field.key) {
            continue;
        }
        if let Some(option) = options.get(*selected) {
            request
                .options
                .insert(field.key.to_string(), option.value.to_string());
        }
    }
    mode.registration
        .iter()
        .cloned()
        .map(|option| LoginField::new(option, true, &request))
        .chain(
            mode.options
                .iter()
                .cloned()
                .map(|option| LoginField::new(option, false, &request)),
        )
        .collect()
}

/// Materialize the core schema for one auth mode. Existing absent choice keys
/// mean the provider's historical implicit first option; this distinction lets
/// RF11 surface a notice when that old implicit value is no longer applicable.
fn default_fields(
    provider: Provider,
    auth_mode: &str,
    existing: Option<&Account>,
) -> (Vec<Field>, Vec<cuotai_core::text::Localized>) {
    let mut fields = Vec::new();
    let mut notices = Vec::new();
    for descriptor in edit::field_descriptors(provider) {
        let (field, notice) = descriptor_to_field(provider, descriptor, auth_mode, existing);
        fields.push(field);
        if let Some(notice) = notice {
            notices.push(notice);
        }
    }
    (fields, notices)
}

fn descriptor_to_field(
    provider: Provider,
    descriptor: &edit::FieldDescriptor,
    auth_mode: &str,
    existing: Option<&Account>,
) -> (Field, Option<cuotai_core::text::Localized>) {
    let existing_value = existing.and_then(|account| account.params.get(descriptor.key));
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
            (field, None)
        }
        edit::FieldDescriptorKind::Choice(choice) => {
            let current = existing.map(|_| {
                existing_value
                    .map(String::as_str)
                    .unwrap_or(choice.options[0].value)
            });
            let resolved = edit::resolve_field_choice(provider, descriptor.key, auth_mode, current)
                .expect("choice descriptor must expose an option for every offered auth mode");
            let mode_options = edit::field_options(provider, descriptor.key, auth_mode)
                .expect("choice descriptor must expose an option for every offered auth mode");
            let selected = mode_options
                .options
                .iter()
                .position(|option| option.index == resolved.option.index)
                .expect("resolved choice belongs to the filtered option list");
            (
                Field::choice(
                    descriptor.key,
                    descriptor.label,
                    descriptor.help,
                    mode_options.options,
                    selected,
                ),
                resolved.notice,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn account(name: &str, env: &str) -> Account {
        Account {
            id: None,
            name: name.into(),
            provider: Provider::OpenAiApi,
            credential_env: Some(env.into()),
            session: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
        }
    }

    #[test]
    fn models_picker_requires_an_explicit_configured_account() {
        let press = |w: &mut Wizard, code| w.handle(KeyEvent::new(code, KeyModifiers::NONE));
        let mut other = account("B", "B_KEY");
        other.provider = Provider::Groq;
        let config = Config {
            accounts: vec![account("A", "A_KEY"), other, account("C", "C_KEY")],
            ..Config::default()
        };
        // OpenAI highlighted: picker lists A and C (indices 0 and 2).
        let mut wizard = Wizard::new(&config, None);
        press(&mut wizard, KeyCode::Char('m'));
        press(&mut wizard, KeyCode::Down);
        assert!(matches!(
            press(&mut wizard, KeyCode::Enter),
            Action::Models { index: 2 }
        ));
        // Provider without configured accounts: Enter queries nothing.
        let empty = Config {
            accounts: Vec::new(),
            ..Config::default()
        };
        let mut wizard = Wizard::new(&empty, None);
        press(&mut wizard, KeyCode::Char('m'));
        assert!(matches!(
            press(&mut wizard, KeyCode::Enter),
            Action::Continue
        ));
        // Esc closes the picker without cancelling the wizard.
        assert!(matches!(press(&mut wizard, KeyCode::Esc), Action::Continue));
        assert_eq!(wizard.step, Step::Provider);
    }

    /// The provider picker offers clean names only, sorted by name, and
    /// leaves out providers without any usage query (core decides).
    #[test]
    fn the_provider_picker_shows_clean_sorted_names_without_access_labels() {
        use ratatui::{backend::TestBackend, Terminal};
        let mut wiz = Wizard::new(&Config::default(), None);
        assert_eq!(wiz.picker, edit::picker_providers(None));
        assert!(!wiz.picker.contains(&Provider::Poolside));
        let names: Vec<_> = wiz
            .picker
            .iter()
            .map(|p| p.label().to_lowercase())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
        assert_eq!(wiz.provider_at_sel(), Provider::OpenAiApi);
        let mut terminal = Terminal::new(TestBackend::new(100, 60)).unwrap();
        terminal
            .draw(|frame| wiz.draw(frame, Path::new("cuotai.toml")))
            .unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(screen.contains(wiz.picker[0].label()), "{screen}");
        for label in [
            "Oficial",
            "Não documentada",
            "Indisponível",
            "Derivada",
            "Official",
            "Undocumented",
            "Unavailable",
            "Derived",
            "No documentada",
            "No disponible",
            "Ufficiale",
            "Non documentato",
            "Non disponibile",
            "Derivato",
        ] {
            assert!(!screen.contains(label), "`{label}`: {screen}");
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

    // PRD 0016 RF2.1: the paste step exists only for session providers, is
    // masked, refuses a paste the core cannot reduce and hands only the
    // reduced cookies to the test and the save.
    #[test]
    fn the_paste_step_hands_only_the_reduced_session_to_the_test_and_the_save() {
        let press = |w: &mut Wizard, code| w.handle(KeyEvent::new(code, KeyModifiers::NONE));
        let to_last_param = |wiz: &mut Wizard| {
            wiz.step = Step::Params;
            wiz.field_idx = wiz.visible_fields().len().saturating_sub(1);
        };
        let mut plain = wizard_with("OpenAI", "OPENAI_ADMIN_KEY", "");
        to_last_param(&mut plain);
        press(&mut plain, KeyCode::Enter);
        assert_eq!(plain.step, Step::Interval);

        let mut mimo = account("MiMo", "XIAOMI_MIMO_API_KEY");
        mimo.provider = Provider::XiaomiMimo;
        let config = Config {
            accounts: vec![mimo],
            ..Config::default()
        };
        let mut wiz = Wizard::new(&config, Some(0));
        to_last_param(&mut wiz);
        press(&mut wiz, KeyCode::Enter);
        assert_eq!(wiz.step, Step::Session);
        assert!(wiz.accepts_paste());

        // A required cookie missing: Enter names it and stays.
        wiz.paste("serviceToken.1=SECRET-PART");
        press(&mut wiz, KeyCode::Enter);
        assert_eq!(wiz.step, Step::Session);
        let error = wiz.error.clone().expect("fragment error");
        assert!(
            error.contains("serviceToken") && !error.contains("SECRET"),
            "{error}"
        );
        let ctrl_u = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL);
        wiz.handle(ctrl_u);

        press(&mut wiz, KeyCode::Down);
        wiz.paste("Cookie: api-platform_serviceToken=SECRET-TOKEN; userId=7; _ga=TRACKER");
        let sessions = match press(&mut wiz, KeyCode::F(4)) {
            Action::TestSession { account, sessions } => {
                assert_eq!(account.provider, Provider::XiaomiMimo);
                sessions
            }
            _ => panic!("F4 tests the paste"),
        };
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].slot(), "platform_session_env");
        assert_eq!(sessions[0].kept(), ["api-platform_serviceToken", "userId"]);
        assert_eq!(sessions[0].discarded(), 1);

        press(&mut wiz, KeyCode::Enter);
        assert_eq!(wiz.step, Step::Interval);
        wiz.step = Step::Summary;
        let summary: String = wiz
            .summary_lines(Path::new("cuotai.toml"))
            .iter()
            .map(|line| line.to_string())
            .collect();
        assert!(
            !summary.contains("SECRET") && !summary.contains("TRACKER"),
            "{summary}"
        );
        // SES-02: Enter tests the paste first; the core's clearance saves.
        let tested = match press(&mut wiz, KeyCode::Enter) {
            Action::TestSession { account, sessions } => {
                assert_eq!(sessions[0].kept(), ["api-platform_serviceToken", "userId"]);
                account
            }
            _ => panic!("Enter tests the paste before saving"),
        };
        match wiz.session_tested(&test_report(&tested, Status::Ready, "")) {
            Some(Action::Save {
                sessions,
                clearance,
                ..
            }) => {
                assert_eq!(sessions.len(), 1);
                assert_eq!(sessions[0].kept(), ["api-platform_serviceToken", "userId"]);
                assert!(clearance.is_some());
            }
            _ => panic!("a passed test saves with the paste"),
        }
    }

    /// The report of a test query of `account`: `status` with `note` first.
    fn test_report(account: &Account, status: Status, note: &'static str) -> Report {
        let notes_l10n = if note.is_empty() {
            Vec::new()
        } else {
            vec![cuotai_core::text::Localized::fixed(note, "x")]
        };
        Report {
            account_id: None,
            diagnostic: None,
            account: account.name.clone(),
            provider: account.provider,
            checked_at: chrono::Utc::now(),
            status,
            metrics: Vec::new(),
            notes: notes_l10n.iter().map(|n| n.fallback.to_string()).collect(),
            notes_l10n,
            source: String::new(),
            session_expires_at: None,
            sub_reports: Vec::new(),
            as_of: None,
        }
    }

    /// A MiMo account at 0 on the summary with a reducible platform paste.
    fn mimo_with_paste() -> Wizard {
        let mut mimo = account("MiMo", "XIAOMI_MIMO_API_KEY");
        mimo.provider = Provider::XiaomiMimo;
        let config = Config {
            accounts: vec![mimo],
            ..Config::default()
        };
        let mut wiz = Wizard::paste_session(&config, 0);
        assert_eq!(wiz.step, Step::Session, "SES-01 opens on the paste step");
        wiz.session_idx = 1;
        wiz.paste("api-platform_serviceToken=SECRET-TOKEN; userId=7");
        wiz.step = Step::Summary;
        wiz
    }

    // SES-02: a rejected or incompatible test blocks the save; a transport
    // failure offers "Save anyway" on the next Enter only.
    #[test]
    fn the_save_waits_for_the_core_test_and_only_a_transport_failure_offers_save_anyway() {
        let press = |w: &mut Wizard, code| w.handle(KeyEvent::new(code, KeyModifiers::NONE));
        let test = |w: &mut Wizard| match press(w, KeyCode::Enter) {
            Action::TestSession { account, .. } => account,
            _ => panic!("the save starts the test"),
        };
        for (status, note) in [
            (Status::AuthenticationError, "user_session.rejected"),
            (Status::Error, "query.schema"),
            (Status::NeedsConfiguration, "query.missing_param"),
        ] {
            let mut wiz = mimo_with_paste();
            let account = test(&mut wiz);
            assert!(wiz
                .session_tested(&test_report(&account, status, note))
                .is_none());
            let error = wiz.error.clone().expect("the refusal is shown");
            assert!(!error.contains("SECRET"), "{error}");
            // No "Save anyway": the next Enter tests again.
            test(&mut wiz);
        }

        let mut wiz = mimo_with_paste();
        let account = test(&mut wiz);
        let unreachable = test_report(&account, Status::Error, "query.transport");
        assert!(wiz.session_tested(&unreachable).is_none());
        assert!(wiz.error.as_deref().unwrap().contains("Enter"));
        // Any other key withdraws "Save anyway".
        press(&mut wiz, KeyCode::Down);
        let account = test(&mut wiz);
        assert!(wiz.session_tested(&unreachable).is_none());
        match press(&mut wiz, KeyCode::Enter) {
            Action::Save {
                sessions,
                clearance,
                edit: Some(0),
                ..
            } => {
                assert_eq!(sessions.len(), 1);
                assert!(clearance.is_some());
            }
            _ => panic!("Enter saves anyway after a transport failure"),
        }

        // A key pressed while the test runs cancels the save it started.
        let mut wiz = mimo_with_paste();
        test(&mut wiz);
        press(&mut wiz, KeyCode::Down);
        assert!(wiz
            .session_tested(&test_report(&account, Status::Ready, ""))
            .is_none());
    }

    // SES-01: only providers with a session open on the paste step.
    #[test]
    fn paste_a_new_session_opens_the_paste_step_of_that_account() {
        let config = Config {
            accounts: vec![account("OpenAI", "OPENAI_ADMIN_KEY")],
            ..Config::default()
        };
        let wiz = Wizard::paste_session(&config, 0);
        assert_eq!(wiz.step, Step::Provider);
        let wiz = mimo_with_paste();
        assert_eq!(wiz.editing, Some(0));
        assert!(!wiz.reduced_sessions().is_empty());
    }

    #[test]
    fn session_migration_needs_a_second_f3_and_leaves_no_variable_in_the_draft() {
        let press = |w: &mut Wizard, code| w.handle(KeyEvent::new(code, KeyModifiers::NONE));
        let mut legacy = account("MiMo", "XIAOMI_MIMO_API_KEY");
        legacy.provider = Provider::XiaomiMimo;
        legacy
            .params
            .insert("platform_session_env".into(), "MIMO_COOKIE".into());
        let config = Config {
            accounts: vec![legacy.clone()],
            ..Config::default()
        };
        let mut wiz = Wizard::new(&config, Some(0));
        wiz.step = Step::Summary;
        // First F3 only explains; any other key withdraws it.
        assert!(matches!(press(&mut wiz, KeyCode::F(3)), Action::Continue));
        assert!(matches!(press(&mut wiz, KeyCode::Down), Action::Continue));
        assert!(matches!(press(&mut wiz, KeyCode::F(3)), Action::Continue));
        assert!(matches!(
            press(&mut wiz, KeyCode::F(3)),
            Action::SessionVault {
                index: 0,
                change: VaultChange::Migrate
            }
        ));
        // The core migrated it: the saved draft keeps the vault reference and
        // no longer names the variable, and F3 has nothing left to offer.
        let mut migrated = legacy;
        migrated.params.remove("platform_session_env");
        migrated.session = Some(user_session::SessionConfig {
            credential_ref: "6f1d3c1e-8a52-4a3c-9f7e-2a8b5b1c0d11".into(),
            slots: ["platform_session_env".to_string()].into(),
        });
        let saved = Config {
            accounts: vec![migrated.clone()],
            ..Config::default()
        };
        wiz.session_migrated(&saved);
        let rebuilt = wiz.build_config().expect("draft still saves");
        assert_eq!(rebuilt.accounts[0].session, migrated.session);
        assert!(!rebuilt.accounts[0]
            .params
            .contains_key("platform_session_env"));
        assert!(matches!(press(&mut wiz, KeyCode::F(3)), Action::Continue));
        assert!(matches!(press(&mut wiz, KeyCode::F(3)), Action::Continue));
    }

    #[test]
    fn forgetting_the_session_needs_a_second_f5_and_the_draft_drops_the_reference() {
        let press = |w: &mut Wizard, code| w.handle(KeyEvent::new(code, KeyModifiers::NONE));
        let mut stored = account("MiMo", "XIAOMI_MIMO_API_KEY");
        stored.provider = Provider::XiaomiMimo;
        stored.session = Some(user_session::SessionConfig {
            credential_ref: "6f1d3c1e-8a52-4a3c-9f7e-2a8b5b1c0d11".into(),
            slots: ["platform_session_env".to_string()].into(),
        });
        let config = Config {
            accounts: vec![stored.clone()],
            ..Config::default()
        };
        let mut wiz = Wizard::new(&config, Some(0));
        wiz.step = Step::Summary;
        assert!(matches!(press(&mut wiz, KeyCode::F(5)), Action::Continue));
        assert!(matches!(press(&mut wiz, KeyCode::Down), Action::Continue));
        assert!(matches!(press(&mut wiz, KeyCode::F(5)), Action::Continue));
        assert!(matches!(
            press(&mut wiz, KeyCode::F(5)),
            Action::SessionVault {
                index: 0,
                change: VaultChange::Forget
            }
        ));
        // The core forgot it: a later save of the draft has no reference and
        // F5 has nothing left to forget.
        let mut forgotten = stored;
        forgotten.session = None;
        wiz.session_forgotten(&Config {
            accounts: vec![forgotten],
            ..Config::default()
        });
        assert_eq!(
            wiz.build_config().expect("draft still saves").accounts[0].session,
            None
        );
        assert!(matches!(press(&mut wiz, KeyCode::F(5)), Action::Continue));
        assert!(matches!(press(&mut wiz, KeyCode::F(5)), Action::Continue));
    }

    #[test]
    fn unavailable_account_saves_only_after_a_second_enter() {
        let press = |w: &mut Wizard, code| w.handle(KeyEvent::new(code, KeyModifiers::NONE));
        let at_summary = |provider| {
            let mut acc = account("A", "SOME_KEY");
            acc.provider = provider;
            let config = Config {
                accounts: vec![acc],
                ..Config::default()
            };
            let mut wiz = Wizard::new(&config, Some(0));
            wiz.step = Step::Summary;
            wiz
        };
        let mut wiz = at_summary(Provider::Poolside);
        assert!(matches!(press(&mut wiz, KeyCode::Enter), Action::Continue));
        assert!(wiz.confirm_save, "the reason is shown before saving");
        // Any other key withdraws the confirmation.
        press(&mut wiz, KeyCode::Down);
        assert!(matches!(press(&mut wiz, KeyCode::Enter), Action::Continue));
        assert!(matches!(
            press(&mut wiz, KeyCode::Enter),
            Action::Save { edit: Some(0), .. }
        ));
        // Every other level saves on the first Enter.
        let mut wiz = at_summary(Provider::OpenAiApi);
        assert!(matches!(
            press(&mut wiz, KeyCode::Enter),
            Action::Save { .. }
        ));
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
                id: None,
                name: "Qwen Cloud".into(),
                provider: Provider::AlibabaTokenPlan,
                credential_env: None,
                session: None,
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
            Action::Login { provider, request }
                if provider == Provider::AlibabaTokenPlan
                    && request.mode == login::MODE_BAILIAN_CONSOLE
                    && request.options.get("console_site").map(String::as_str) == Some("international")
                    && request.options.get("profile").map(String::as_str) == Some("qwen-personal")
        ));
        wiz.external_login_finished(BTreeMap::from([(
            "cli_profile".into(),
            "qwen-personal".into(),
        )]));
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
    fn f2_reauth_reuses_existing_credential_id() {
        let mut oauth = OAuthConfig::new(String::new());
        oauth.options.insert("region".into(), "us-east-1".into());
        let credential_id = oauth.credential_id.clone();
        let config = Config {
            accounts: vec![Account {
                id: None,
                name: "Kiro".into(),
                provider: Provider::Kiro,
                credential_env: None,
                session: None,
                oauth: Some(oauth),
                params: BTreeMap::new(),
                refresh_seconds: None,
            }],
            ..Config::default()
        };
        let mut wiz = Wizard::new(&config, Some(0));
        wiz.step = Step::Credential;
        let action = wiz.handle(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
        let Action::Login { request, .. } = action else {
            panic!("F2 deve iniciar o modo genérico");
        };
        assert_eq!(request.mode, login::MODE_OAUTH);
        assert_eq!(request.credential_id(), Some(credential_id.as_str()));
        assert_eq!(
            request.options.get("region").map(String::as_str),
            Some("us-east-1")
        );
    }

    #[test]
    fn wizard_visible_fields_match_core_visibility_decision() {
        // The wizard MUST NOT carry its own provider-specific visibility
        // rules. After the user flips the Grok product to "cli", the
        // session_env field becomes visible and team_id disappears — both
        // must agree with `edit::visible_field_indices`.
        let mut wiz = Wizard::new(&Config::default(), None);
        wiz.provider = Provider::Grok;
        wiz.fields = default_fields(Provider::Grok, edit::AUTH_MODE_CREDENTIAL, None).0;
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
                    id: None,
                    name: "Other".into(),
                    provider: Provider::AnthropicApi,
                    credential_env: Some("ANTHROPIC_ADMIN_KEY".into()),
                    session: None,
                    oauth: None,
                    params: BTreeMap::new(),
                    refresh_seconds: Some(45),
                },
                Account {
                    id: None,
                    name: "OpenAI".into(),
                    provider: Provider::OpenAiApi,
                    credential_env: Some("OPENAI_ADMIN_KEY".into()),
                    session: None,
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
                id: None,
                name: "Prod".into(),
                provider: Provider::AwsBedrock,
                credential_env: None,
                session: None,
                oauth: None,
                params: BTreeMap::from([("profile".into(), "default".into())]),
                refresh_seconds: None,
            }],
            ..Config::default()
        };
        let mut wiz = Wizard::new(&config, Some(0));
        wiz.step = Step::Credential;
        assert_eq!(wiz.credential, "default");
        assert_eq!(wiz.auth_mode, "credential");
        assert_eq!(wiz.auth_choice_count(), 2);
        assert!(!wiz.oauth_active());
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
                id: None,
                name: "Qwen".into(),
                provider: Provider::AlibabaTokenPlan,
                credential_env: None,
                session: None,
                oauth: None,
                params: BTreeMap::from([("cli_profile".into(), "qwen-personal".into())]),
                refresh_seconds: None,
            }],
            ..Config::default()
        };
        let mut wiz = Wizard::new(&config, Some(0));
        wiz.step = Step::Credential;
        assert_eq!(wiz.credential, "qwen-personal");
        assert_eq!(wiz.auth_mode, login::MODE_BAILIAN_CONSOLE);
        assert_eq!(wiz.validate_step(), Ok(()));
    }

    #[test]
    fn wizard_tab_cycles_only_modes_discovered_by_core() {
        for provider in Provider::ALL {
            let config = Config {
                accounts: vec![Account {
                    id: None,
                    name: "Test".into(),
                    provider,
                    credential_env: None,
                    session: None,
                    oauth: None,
                    params: BTreeMap::new(),
                    refresh_seconds: None,
                }],
                ..Config::default()
            };
            let mut wiz = Wizard::new(&config, Some(0));
            wiz.step = Step::Credential;
            let before = wiz.auth_mode.clone();
            let count = wiz.auth_choice_count();
            let action = wiz.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
            assert!(matches!(action, Action::Continue));
            if count > 1 {
                assert_ne!(wiz.auth_mode, before, "{provider:?} should cycle mode");
            } else {
                assert_eq!(wiz.auth_mode, before, "{provider:?} has no alternate mode");
            }
            if let Some(mode) = wiz.selected_mode() {
                assert!(login::login_modes(provider)
                    .modes
                    .iter()
                    .any(|offered| offered.id == mode.id));
            }
        }
    }
    #[test]
    fn kiro_mode_switch_reconciles_shared_choice_and_shows_notice() {
        let config = Config {
            accounts: vec![Account {
                id: None,
                name: "Kiro".into(),
                provider: Provider::Kiro,
                credential_env: None,
                session: None,
                oauth: Some(OAuthConfig::new(String::new())),
                params: BTreeMap::from([("auth_method".into(), "builder-id".into())]),
                refresh_seconds: None,
            }],
            ..Config::default()
        };
        let mut wizard = Wizard::new(&config, Some(0));
        assert_eq!(wizard.auth_mode, login::MODE_OAUTH);

        wizard.select_auth_choice(0);

        assert_eq!(wizard.auth_mode, edit::AUTH_MODE_CREDENTIAL);
        let auth_method = wizard
            .fields
            .iter()
            .find(|field| field.key == "auth_method")
            .expect("Kiro auth_method field");
        let FieldKind::Choice { options, selected } = &auth_method.kind else {
            panic!("auth_method must be a choice");
        };
        assert_eq!(options[*selected].value, "google");
        assert!(
            wizard
                .notice
                .as_deref()
                .is_some_and(|notice| notice.contains("substituído por Google")),
            "mode switch must visibly explain the substitution"
        );
    }

    #[test]
    fn editing_old_invalid_kiro_combination_substitutes_with_notice() {
        let config = Config {
            accounts: vec![Account {
                id: None,
                name: "Kiro".into(),
                provider: Provider::Kiro,
                credential_env: Some("KIRO_ACCESS_TOKEN".into()),
                session: None,
                oauth: None,
                params: BTreeMap::new(),
                refresh_seconds: None,
            }],
            ..Config::default()
        };
        let wizard = Wizard::new(&config, Some(0));
        let auth_method = wizard
            .fields
            .iter()
            .find(|field| field.key == "auth_method")
            .expect("Kiro auth_method field");
        let FieldKind::Choice { options, selected } = &auth_method.kind else {
            panic!("auth_method must be a choice");
        };
        assert_eq!(options[*selected].value, "google");
        assert!(wizard.notice.is_some());
    }
}
