//! TUI login dialog. All login policy and orchestration live in the
//! core (`cuotai_core::login`, RF2); this module only renders the typed
//! events and forwards the code pasted by the user.

use crate::l10n::{tr, trf};
use crate::palette;
use crate::ui::{resolve_l10n, safe};
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

// Local Dracula palette — same as the dashboard/wizard.
const DRACULA_BG: Color = palette::BG;
const DRACULA_FG: Color = palette::FG;
const DRACULA_CYAN: Color = palette::ACCENT;

pub(crate) enum Outcome {
    /// Cuotai credential authorized; the token is memory-only until commit.
    Authorized(CompletedLogin),
    /// External CLI completed; only public account params are returned.
    External(ExternalLogin),
    /// Credential written to the vault; only the TOML write is missing.
    CredentialReady(CredentialCommit),
    /// TOML saved and post-save cleanup resolved (possibly non-fatally).
    Committed(LoginCommitOutcome),
    /// The TOML write failed after the credential was written; the
    /// core's compensation has already been applied and the exact state is reported.
    RolledBack(PartialFailure),
}

enum Job {
    /// Interactive flow driven by the core's driver.
    Login(LoginDriver),
    /// Commit step 1 (vault). Not cancellable.
    Commit(JoinHandle<Result<CredentialCommit, OAuthError>>),
    /// TOML already saved; cleanup of the replaced key in progress.
    Confirm(JoinHandle<LoginCommitOutcome>),
    /// Compensation after the TOML write failed; also not cancellable.
    Rollback(JoinHandle<PartialFailure>),
    /// Result already collected.
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
    /// Wraps a `LoginDriver` already started by the core
    /// (`cuotai_core::login::start_login`).
    pub fn new(driver: LoginDriver) -> Self {
        Self {
            job: Job::Login(driver),
            lines: Vec::new(),
            scroll: 0,
            code: String::new(),
            code_status: paste_code_hint().into(),
        }
    }

    /// Commit step 1 of RF2.4: captures the previous token (reauth) and writes the
    /// credential to the vault. The TOML is written afterwards by the caller, followed by
    /// `CredentialCommit::confirm` or `LoginTask::rollback`.
    pub fn begin_commit(session: Arc<OAuthSession>, login: CompletedLogin) -> Self {
        let job = tokio::spawn(async move {
            cuotai_core::login::begin_credential_commit(&session, &login).await
        });
        Self {
            job: Job::Commit(job),
            lines: vec![tr(
                "login.saving_vault",
                "Saving the credentials in the system vault. Authorize Keychain access if asked.",
            )
            .into()],
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
            lines: vec![tr(
                "login.checking_replaced",
                "Configuration saved; checking the replaced credential...",
            )
            .into()],
            scroll: 0,
            code: String::new(),
            code_status: String::new(),
        }
    }
    /// Commit compensation when the TOML write failed: the core
    /// restores the previous token (reauth) or removes the new token, and returns
    /// the exact state to the UI.
    pub fn rollback(session: Arc<OAuthSession>, commit: CredentialCommit, message: String) -> Self {
        let job = tokio::spawn(async move { commit.rollback(&session, message).await });
        Self {
            job: Job::Rollback(job),
            lines: vec![tr(
                "login.rolling_back",
                "The configuration could not be saved; undoing the vault write...",
            )
            .into()],
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

    /// Drains the core's typed events into the dialog lines.
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
                    instructions_l10n,
                } => {
                    self.lines = vec![
                        tr(
                            "login.authorize_in_browser",
                            "Authorize in the system browser:",
                        )
                        .into(),
                        url,
                    ];
                    if let Some(code) = user_code {
                        self.lines.push(trf(
                            "login.user_code",
                            "Authorization code: {code}",
                            &[("code", &code)],
                        ));
                    }
                    self.lines.push(localized(instructions, instructions_l10n));
                }
                LoginEvent::Progress { text, l10n } => self.lines.push(localized(text, l10n)),
                LoginEvent::WaitingCode => self.code_status = paste_code_hint().into(),
                LoginEvent::CodeRejected { message, l10n } => {
                    self.code_status = localized(message, l10n)
                }
                LoginEvent::BrowserUnavailable { .. } => self.lines.push(
                    tr(
                        "login.browser_unavailable",
                        "Could not open the browser; open the URL above manually.",
                    )
                    .into(),
                ),
                LoginEvent::Finished { .. } => {}
                LoginEvent::Failed { message, l10n } => self.lines.push(localized(message, l10n)),
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

    /// Only interactive flows can be cancelled (Esc). During the
    /// commit/rollback the write proceeds to the end so as never to leave the vault
    /// and the TOML in an ambiguous state.
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
            self.code_status = tr(
                "login.code.too_long",
                "Code too long. Ctrl-U clears the field.",
            )
            .into();
            return;
        }
        if !compact
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '_' | '-' | '='))
        {
            self.code_status = tr(
                "login.code.only_code",
                "Paste only the authentication code MiMo shows.",
            )
            .into();
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
                crate::dashboard::pop_grapheme(&mut self.code);
            }
            KeyCode::Enter if !self.code.is_empty() => {
                let code = std::mem::take(&mut self.code);
                let Job::Login(driver) = &self.job else {
                    return;
                };
                match driver.submit_code(code) {
                    Ok(()) => {
                        self.code_status = tr(
                            "login.code.validating",
                            "Validating the code in the pending session...",
                        )
                        .into()
                    }
                    Err(SubmitCodeError::Full(code)) => {
                        self.code = code;
                        self.code_status = tr(
                            "login.code.wait",
                            "Wait for the previous validation before sending again.",
                        )
                        .into();
                    }
                    Err(SubmitCodeError::Closed) => {
                        self.code_status =
                            tr("login.code.closed", "The login session has already ended.").into()
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
        // Dracula frame background, same palette as the dashboard/wizard.
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
            .title(tr("login.title", " OAuth login "));
        let inner = block.inner(areas[0]);
        let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
        let max = paragraph
            .line_count(inner.width)
            .saturating_sub(usize::from(inner.height))
            .min(usize::from(u16::MAX)) as u16;
        self.scroll = self.scroll.min(max);
        frame.render_widget(paragraph.scroll((self.scroll, 0)).block(block), areas[0]);
        let footer = if self.accepts_code() {
            trf(
                "login.code.footer",
                "MiMo code (hidden): {mask} [{count} characters]\n{status}\nEnter sends • Ctrl-U clears • Esc cancels • ↑↓ scroll the instructions",
                &[
                    ("mask", &"*".repeat(self.code.len().min(32))),
                    ("count", &self.code.len()),
                    ("status", &self.code_status),
                ],
            )
        } else if matches!(
            self.job,
            Job::Commit(_) | Job::Confirm(_) | Job::Rollback(_)
        ) {
            tr(
                "login.footer.saving",
                "Wait for the safe write. Ctrl-C quits; do not cancel during the compensation.",
            )
            .into()
        } else {
            tr(
                "login.footer.flow",
                "↑↓ scroll • Esc cancels and returns to the wizard • Ctrl-C quits",
            )
            .into()
        };
        frame.render_widget(
            Paragraph::new(footer)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(DRACULA_CYAN)),
            areas[1],
        );
    }
}

/// RF2.4 partial failure text: the UI only presents the exact state
/// reported by the core, without inventing policy.
pub(crate) fn partial_failure_message(failure: &PartialFailure) -> String {
    use cuotai_core::login::{CommitStage, Compensation};
    let base = trf(
        "login.partial.base",
        "Could not save the login: {message}",
        &[("message", &failure.message)],
    );
    let detail = match (failure.stage, failure.compensation) {
        (CommitStage::Vault, _) => tr(
            "login.partial.vault",
            "Nothing was saved: neither the credential in the vault nor the configuration. Try again.",
        ),
        (_, Compensation::PreviousRestored) => tr(
            "login.partial.previous_restored",
            "The configuration was NOT saved and the previous token was restored in the vault. The account keeps using the previous login; fix the problem and save again.",
        ),
        (_, Compensation::NewRemoved) => tr(
            "login.partial.new_removed",
            "The configuration was NOT saved and the new credential was removed from the vault. Fix the problem and try again.",
        ),
        (_, Compensation::Failed) => tr(
            "login.partial.failed",
            "The configuration was NOT saved, but the credential stays in the vault and could not be undone. Saving again reuses that credential (same key); to discard it, remove the account and log in again.",
        ),
        (_, Compensation::NotNeeded) => tr(
            "login.partial.not_needed",
            "Writing the TOML is what failed; the vault needed no compensation.",
        ),
    };
    format!("{base}\n\n{detail}")
}

/// Truthful non-fatal outcome: both the new config and credential were saved,
/// but the replaced token could not be removed.
pub(crate) fn cleanup_failure_message(failure: &CleanupFailure) -> String {
    trf(
        "login.cleanup_warning",
        "Login saved with a warning: {message}\n\nThe account already uses the new credential; the previous one was kept in the vault.",
        &[("message", &failure.message)],
    )
}

/// "Paste this attempt's code and press Enter."
fn paste_code_hint() -> &'static str {
    tr(
        "login.code.paste",
        "Paste the code of this attempt and press Enter.",
    )
}

/// A login line in the TUI locale: its id when the core gave one, the
/// English text otherwise (lines relayed from a provider CLI).
fn localized(text: String, note: Option<cuotai_core::text::Localized>) -> String {
    note.map_or(text, |note| resolve_l10n(&note))
}
