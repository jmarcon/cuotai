//! PRD 0050 RF2: the Rust-owned confirmation before any action that saves,
//! approves, enables or runs a local command (hooks and extensions).
//!
//! The webview asks for the action only; it supplies neither the words of
//! the question nor the answer. Rust builds the dialog from the core's
//! typed review ([`cuotai_core::execution_review`], [`Hooks::review`],
//! `Manager::review`), shows it as an operating-system dialog outside the
//! webview DOM, and acts only on the OS answer. Cancel, a closed dialog or a
//! failure to show it is the fixed `execution.declined` error: nothing is
//! written, approved, enabled or spawned. One [`Gate`] serializes the
//! confirmations, so concurrent clicks never share or replay an answer.
//!
//! The transactions ([`save_hooks`], [`approve_hooks`], [`enable_hooks`],
//! [`test_hook`], [`ask_extension`]) take any [`NativeConfirmer`], so the
//! tests drive them with a fake that answers, cancels or edits the file
//! while the dialog is open.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use cuotai_core::execution_review::{self, Review};
use cuotai_core::hooks::{self, Document, Hooks, RuleDraft};
use cuotai_core::text::Localized;

/// The question shown to the user, already rendered in their language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub title: String,
    pub body: String,
    pub accept: String,
    pub cancel: String,
}

impl Question {
    /// Render `review` with its `lines` in `locale`.
    pub fn new(review: Review, lines: &[Localized], locale: &str) -> Self {
        let mut body = vec![review.intro().resolve(locale), String::new()];
        body.extend(lines.iter().map(|line| line.resolve(locale)));
        Self {
            title: review.title().resolve(locale),
            body: body.join("\n"),
            accept: review.accept().resolve(locale),
            cancel: execution_review::cancel().resolve(locale),
        }
    }
}

/// Shows a [`Question`] and answers `true` only for the accepting button.
pub trait NativeConfirmer: Send + Sync {
    fn ask<'a>(&'a self, question: Question) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>>;
}

/// The operating-system dialog of `tauri-plugin-dialog`, created by Rust.
pub struct DialogConfirmer {
    app: tauri::AppHandle,
}

impl DialogConfirmer {
    pub fn new(app: &tauri::AppHandle) -> Self {
        Self { app: app.clone() }
    }
}

impl NativeConfirmer for DialogConfirmer {
    fn ask<'a>(&'a self, question: Question) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel();
            self.app
                .dialog()
                .message(question.body)
                .title(question.title)
                .kind(MessageDialogKind::Warning)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    question.accept,
                    question.cancel,
                ))
                .show(move |accepted| {
                    let _ = tx.send(accepted);
                });
            // A dialog that could not be shown drops its sender: declined.
            rx.await.unwrap_or(false)
        })
    }
}

/// One confirmation at a time: a second request while a dialog is open is
/// refused with [`execution_review::pending`] and asks nothing.
#[derive(Default)]
pub struct Gate(tokio::sync::Mutex<()>);

impl Gate {
    pub fn enter(&self) -> Result<tokio::sync::MutexGuard<'_, ()>, Localized> {
        self.0.try_lock().map_err(|_| execution_review::pending())
    }
}

/// The language of the desktop (`desktop.json`), for the dialog texts.
pub fn locale() -> String {
    crate::settings::settings_path()
        .and_then(|file| crate::settings::read_locale(&file))
        .unwrap_or_else(|| "en-US".to_owned())
}

async fn confirmed(
    confirmer: &dyn NativeConfirmer,
    review: Review,
    lines: &[Localized],
    locale: &str,
) -> Result<(), Localized> {
    if confirmer.ask(Question::new(review, lines, locale)).await {
        Ok(())
    } else {
        Err(execution_review::declined())
    }
}

/// RF2.2: validate `drafts` in the core, show them natively, then save
/// over `revision`. Saving never approves.
pub async fn save_hooks(
    confirmer: &dyn NativeConfirmer,
    locale: &str,
    path: &Path,
    revision: Option<&str>,
    drafts: Vec<RuleDraft>,
) -> Result<(), Localized> {
    let shown = Hooks::from_drafts(path.to_path_buf(), drafts.clone())?;
    confirmed(confirmer, Review::SaveHooks, &shown.review(), locale).await?;
    Document::save(path, revision, drafts)?;
    Ok(())
}

/// RF2.2: read the rules of `path`, show them natively, and return the
/// digest of what was shown; the caller's commit
/// ([`crate::hooks::Integration::confirm`]) re-reads the file and refuses
/// rules changed meanwhile.
pub async fn approve_hooks(
    confirmer: &dyn NativeConfirmer,
    locale: &str,
    path: &Path,
) -> Result<String, Localized> {
    let shown = Document::read(path).into_hooks()?;
    confirmed(confirmer, Review::ApproveHooks, &shown.review(), locale).await?;
    Ok(shown.digest())
}

/// RF2.3: before turning hooks on, show the approved rules (`approved`, the
/// ones that would start) natively. `None` (nothing approved, so nothing
/// would run) asks nothing.
pub async fn enable_hooks(
    confirmer: &dyn NativeConfirmer,
    locale: &str,
    approved: Option<&Hooks>,
) -> Result<(), Localized> {
    match approved {
        Some(hooks) => confirmed(confirmer, Review::EnableHooks, &hooks.review(), locale).await,
        None => Ok(()),
    }
}

/// RF2.3: show the approved rule `name` natively and return the digest of
/// the rules it belongs to; the caller's run
/// ([`crate::hooks::Integration::test_checked`]) must find exactly them
/// still active.
pub async fn test_hook(
    confirmer: &dyn NativeConfirmer,
    locale: &str,
    approved: &Hooks,
    name: &str,
) -> Result<String, Localized> {
    let lines = approved.rule_review(name).ok_or_else(unknown_hook)?;
    confirmed(confirmer, Review::TestHook, &lines, locale).await?;
    Ok(approved.digest())
}

/// RF2.5: show an extension's review natively before `review` is acted on.
pub async fn ask_extension(
    confirmer: &dyn NativeConfirmer,
    locale: &str,
    review: Review,
    lines: &[Localized],
) -> Result<(), Localized> {
    confirmed(confirmer, review, lines, locale).await
}

/// The rule to test is not among the approved ones.
pub fn unknown_hook() -> Localized {
    hooks::changed()
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use std::sync::Mutex;

    /// Answers with `accept` after running `during` (a file edit, say),
    /// recording every question.
    pub struct Fake {
        pub accept: bool,
        pub asked: Mutex<Vec<Question>>,
        pub during: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    impl Fake {
        pub fn new(accept: bool) -> Self {
            Self {
                accept,
                asked: Mutex::new(Vec::new()),
                during: Mutex::new(None),
            }
        }

        pub fn while_open(self, during: impl FnOnce() + Send + 'static) -> Self {
            *self.during.lock().unwrap() = Some(Box::new(during));
            self
        }

        pub fn questions(&self) -> usize {
            self.asked.lock().unwrap().len()
        }
    }

    impl NativeConfirmer for Fake {
        fn ask<'a>(
            &'a self,
            question: Question,
        ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
            self.asked.lock().unwrap().push(question);
            if let Some(during) = self.during.lock().unwrap().take() {
                during();
            }
            let accept = self.accept;
            Box::pin(async move { accept })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::Fake;
    use super::*;

    fn draft(name: &str, command: &[&str]) -> RuleDraft {
        serde_json::from_value(serde_json::json!({
            "name": name,
            "events": ["quota_exhausted"],
            "command": command,
        }))
        .unwrap()
    }

    fn write(path: &Path, drafts: Vec<RuleDraft>) {
        let revision = Document::read(path).revision().map(str::to_owned);
        Document::save(path, revision.as_deref(), drafts).unwrap();
    }

    #[tokio::test]
    async fn a_question_is_built_in_rust_from_the_core_review() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.toml");
        let fake = Fake::new(true);
        save_hooks(
            &fake,
            "en-US",
            &path,
            None,
            vec![draft("notify", &["notify-tool", "low"])],
        )
        .await
        .unwrap();
        let question = fake.asked.lock().unwrap()[0].clone();
        assert_eq!(question.title, "Save commands");
        assert_eq!(question.accept, "Save");
        assert!(
            question.body.contains(r#"command: ["notify-tool","low"]"#),
            "{}",
            question.body
        );
        assert!(
            question.body.contains("PATH, HOME, LANG, CUOTAI_EVENT"),
            "{}",
            question.body
        );
        assert!(
            question.body.contains("working directory:"),
            "{}",
            question.body
        );
        // Translated from the core catalogs.
        let pt = Question::new(Review::ApproveHooks, &[], "pt-BR");
        assert_ne!(pt.title, "Approve commands");
    }

    // security-invariant: SEC-NATIVE-CONFIRM
    #[tokio::test]
    async fn cancel_saves_approves_enables_and_runs_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.toml");
        let declined = execution_review::declined();

        let cancel = Fake::new(false);
        let error = save_hooks(
            &cancel,
            "en-US",
            &path,
            None,
            vec![draft("a", &["true-tool"])],
        )
        .await
        .unwrap_err();
        assert_eq!(error, declined);
        assert!(!path.exists(), "nothing written");

        write(&path, vec![draft("a", &["true-tool"])]);
        let error = approve_hooks(&Fake::new(false), "en-US", &path)
            .await
            .unwrap_err();
        assert_eq!(error, declined, "no digest to approve");

        let hooks = Document::read(&path).into_hooks().unwrap();
        assert_eq!(
            enable_hooks(&Fake::new(false), "en-US", Some(&hooks)).await,
            Err(declined.clone())
        );
        let none = Fake::new(false);
        assert_eq!(enable_hooks(&none, "en-US", None).await, Ok(()));
        assert_eq!(none.questions(), 0, "nothing approved: nothing to ask");

        let error = test_hook(&Fake::new(false), "en-US", &hooks, "a")
            .await
            .unwrap_err();
        assert_eq!(error, declined, "no digest to run");
        assert_eq!(
            test_hook(&Fake::new(true), "en-US", &hooks, "missing").await,
            Err(unknown_hook())
        );
        assert_eq!(
            ask_extension(&Fake::new(false), "en-US", Review::EnableExtension, &[]).await,
            Err(declined)
        );
    }

    /// The file changes while the dialog is open: the approval names the
    /// digest of what was shown and the commit, which re-reads the file,
    /// refuses it.
    // security-invariant: SEC-NATIVE-CONFIRM
    #[tokio::test]
    async fn a_file_changed_while_the_dialog_is_open_is_not_approved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.toml");
        write(&path, vec![draft("a", &["echo-tool", "shown"])]);
        let swapped = path.clone();
        let fake = Fake::new(true).while_open(move || {
            write(&swapped, vec![draft("a", &["sh-tool", "-c", "unseen"])]);
        });
        let mut integration = crate::hooks::Integration::new(
            true,
            Some(hooks::Consent::at(dir.path().join("consent.json"))),
            None,
        );
        let digest = approve_hooks(&fake, "en-US", &path).await.unwrap();
        let error = integration.confirm(Some(&path), &digest).unwrap_err();
        assert_eq!(error, hooks::changed());
        assert!(!integration.view(Some(&path)).confirmed);

        // A save over a file changed while its dialog was open is refused too.
        let revision = Document::read(&path).revision().map(str::to_owned);
        let swapped = path.clone();
        let fake = Fake::new(true).while_open(move || {
            write(&swapped, vec![draft("b", &["true-tool"])]);
        });
        let error = save_hooks(
            &fake,
            "en-US",
            &path,
            revision.as_deref(),
            vec![draft("c", &["true-tool"])],
        )
        .await
        .unwrap_err();
        assert_eq!(error, hooks::changed());
    }

    #[tokio::test]
    async fn an_accepted_approval_commits_exactly_what_was_shown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.toml");
        write(&path, vec![draft("a", &["echo-tool", "ok"])]);
        let mut integration = crate::hooks::Integration::new(
            true,
            Some(hooks::Consent::at(dir.path().join("consent.json"))),
            None,
        );
        let digest = approve_hooks(&Fake::new(true), "en-US", &path)
            .await
            .unwrap();
        integration.confirm(Some(&path), &digest).unwrap();
        assert!(integration.view(Some(&path)).confirmed);
        // The approved set is what turning on and a test show and pin.
        let approved = integration.approved(Some(&path)).unwrap();
        assert_eq!(approved.digest(), digest);
        let fake = Fake::new(true);
        enable_hooks(&fake, "en-US", Some(&approved)).await.unwrap();
        assert_eq!(fake.asked.lock().unwrap()[0].title, "Turn on hooks");
        // A test pinned to an older digest runs nothing.
        assert_eq!(
            integration.test_checked(Some(&path), "a", "stale"),
            Err("hook_unknown")
        );
    }

    #[tokio::test]
    async fn a_second_confirmation_waits_for_none_and_is_refused() {
        let gate = Gate::default();
        let open = gate.enter().unwrap();
        assert_eq!(gate.enter().err(), Some(execution_review::pending()));
        drop(open);
        assert!(gate.enter().is_ok());
    }
}
