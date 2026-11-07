//! PRD 0044 RF5.2: the notification channels from the command line:
//! `--channel-add <kind> --label L [--filter key=value]… [--sign]`,
//! `--channel-list`, `--channel-remove <id>`, `--channel-test <id>` and
//! `--channel-log`.
//!
//! The URL is read from standard input or a prompt, never from an argument
//! (so it never enters the shell history), and goes to the vault; the
//! consent text is printed before anything is saved, and saving needs a
//! confirmation typed in a terminal (or `--yes` when the URL is piped).
//! Nothing here prints a URL: the host and a masked form only.

use crate::l10n::{tr, trf};
use chrono::Utc;
use cuotai_core::channels::{
    self, delivery, log::DeliveryLog, ChannelKind, Dispatcher, Draft, Filters, Outcome, Vault,
};
use cuotai_core::notify_prefs;
use cuotai_core::oauth::OAuthSession;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Arc;

type Error = Box<dyn std::error::Error>;

fn locale() -> &'static str {
    crate::ui::tui_locale()
}

fn settings() -> Result<PathBuf, Error> {
    notify_prefs::settings_path().ok_or_else(|| {
        tr(
            "channels.cli.no_settings",
            "No configuration folder on this system, so channels cannot be kept.",
        )
        .into()
    })
}

/// PRD 0032 RF7.1: whether texts are private (`desktop.json`).
fn private() -> bool {
    notify_prefs::Source::open().loaded().notify.private
}

/// PRD 0044 RF5.3: the dispatcher of `--notify` (and of the commands
/// below), over the session's vault; `None` without a configuration
/// folder or outside a runtime.
pub(crate) fn dispatcher(session: &Arc<OAuthSession>) -> Option<Dispatcher> {
    let settings = notify_prefs::settings_path()?;
    Dispatcher::new(settings, Vault::new(session.token_store()), Utc::now())
}

fn localized(error: cuotai_core::text::Localized) -> Error {
    error.resolve(locale()).into()
}

/// What `--channel-add` was given.
pub(crate) struct AddArgs<'a> {
    pub kind: &'a str,
    pub label: Option<&'a str>,
    pub filters: &'a [String],
    pub sign: bool,
    pub yes: bool,
}

/// `--channel-add`.
pub(crate) fn add(session: &OAuthSession, args: AddArgs<'_>) -> Result<u8, Error> {
    let terminal = io::stdin().is_terminal() && io::stderr().is_terminal();
    add_with(
        &settings()?,
        &Vault::new(session.token_store()),
        &args,
        &mut io::stdin().lock(),
        terminal,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )
}

/// [`add`] over explicit streams: the URL is the first line of `input`
/// (after a prompt on a `terminal`), the confirmation the next one.
fn add_with(
    settings: &std::path::Path,
    vault: &Vault,
    args: &AddArgs<'_>,
    input: &mut dyn BufRead,
    terminal: bool,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<u8, Error> {
    let (kind, label, filters, sign, yes) =
        (args.kind, args.label, args.filters, args.sign, args.yes);
    let kind = ChannelKind::parse(kind).ok_or_else(|| -> Error {
        tr(
            "channels.cli.kind",
            "The kind is webhook, ntfy, slack or discord.",
        )
        .into()
    })?;
    let label = label.ok_or_else(|| -> Error {
        tr("channels.cli.label", "--channel-add needs --label NAME.").into()
    })?;
    let mut chosen = Filters::default();
    for filter in filters {
        chosen.apply(filter).map_err(localized)?;
    }
    let draft = Draft {
        kind,
        label: label.to_owned(),
        filters: chosen,
        sign,
    };
    channels::validate_label(&draft.label).map_err(localized)?;
    if terminal {
        write!(
            stderr,
            "{}",
            tr(
                "channels.cli.prompt",
                "Paste the address (it stays out of your shell history): "
            )
        )?;
        stderr.flush()?;
    }
    let mut url = String::new();
    input.read_line(&mut url)?;
    let url = zeroizing(url);
    let consent = channels::consent(&draft, url.trim(), private(), locale(), Utc::now())
        .map_err(localized)?;
    writeln!(
        stderr,
        "\n{}\n  {}: {}\n  {}: {}\n  {}:\n    {}",
        trf(
            "channels.cli.consent_title",
            "New {kind} channel \"{label}\"",
            &[
                ("kind", &kind.name()),
                ("label", &crate::ui::safe(&draft.label))
            ]
        ),
        tr("channels.cli.host", "Address"),
        consent.masked,
        tr("channels.cli.filters", "Sends"),
        consent.filters.resolve(locale()),
        tr("channels.cli.example", "Example"),
        consent.example.replace('\n', "\n    "),
    )?;
    if let Some(warning) = &consent.warning {
        writeln!(stderr, "  ! {}", warning.resolve(locale()))?;
    }
    writeln!(stderr, "\n{}", consent.sentence.resolve(locale()))?;
    if !yes {
        if !terminal {
            return Err(tr(
                "channels.cli.needs_terminal",
                "Saving a channel asks for a confirmation in a terminal; with the address on standard input, pass --yes after reading the text above.",
            )
            .into());
        }
        write!(
            stderr,
            "{}",
            tr("channels.cli.confirm", "Save this channel? [y/N] ")
        )?;
        stderr.flush()?;
        let mut answer = String::new();
        input.read_line(&mut answer)?;
        if !answer.trim().eq_ignore_ascii_case("y") {
            return Err(tr("channels.cli.cancelled", "Nothing was saved.").into());
        }
    }
    let saved =
        channels::add(settings, vault, &draft, url.trim(), Utc::now()).map_err(localized)?;
    writeln!(
        stdout,
        "{}",
        trf(
            "channels.cli.saved",
            "Channel {id} saved and on. Try it with cuotai --channel-test {id}.",
            &[("id", &saved.channel.id)]
        )
    )?;
    if let Some(secret) = &saved.signing_secret {
        // Shown once (§12): the receiver needs it to check the signature.
        writeln!(
            stdout,
            "{}\n  {secret}",
            tr(
                "channels.cli.secret",
                "Signing secret (shown only now; X-Cuotai-Signature is the HMAC-SHA256 of each body with it):"
            )
        )?;
    }
    Ok(0)
}

/// The URL as typed, overwritten when dropped.
struct Zeroizing(String);

impl std::ops::Deref for Zeroizing {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl Drop for Zeroizing {
    fn drop(&mut self) {
        // SAFETY-free overwrite: replace every byte with a space (valid UTF-8).
        let len = self.0.len();
        self.0.clear();
        self.0.extend(std::iter::repeat_n(' ', len));
    }
}

fn zeroizing(text: String) -> Zeroizing {
    Zeroizing(text)
}

/// `--channel-list [--json]`.
pub(crate) fn list(json: bool) -> Result<u8, Error> {
    let log = DeliveryLog::in_data_dir();
    let views = delivery::views(&settings()?, log.as_ref(), private(), Utc::now());
    let mut stdout = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(
            &mut stdout,
            &serde_json::json!({ "schema": 1, "channels": views }),
        )?;
        writeln!(stdout)?;
        return Ok(0);
    }
    writeln!(
        stdout,
        "{}",
        tr("channels.cli.list_title", "CUOTAI • notification channels")
    )?;
    if views.is_empty() {
        writeln!(
            stdout,
            "\n{}",
            tr(
                "channels.cli.none",
                "No channel. Add one with cuotai --channel-add <webhook|ntfy|slack|discord> --label NAME."
            )
        )?;
        return Ok(0);
    }
    for view in &views {
        let channel = &view.channel;
        let state = if channel.enabled {
            tr("channels.cli.on", "on")
        } else {
            tr("channels.cli.off", "off")
        };
        let last = match &view.last {
            None => tr("channels.cli.never", "nothing sent yet").to_owned(),
            Some(last) => {
                let status = last.status.map(|s| s.to_string()).unwrap_or_default();
                let outcome = match last.outcome {
                    channels::log::LogOutcome::Delivered => {
                        tr("channels.cli.delivered", "delivered")
                    }
                    _ => tr("channels.cli.failed", "failed"),
                };
                format!("{outcome} {status} · {}", crate::ui::local_time(last.at))
            }
        };
        writeln!(
            stdout,
            "\n{}  {} ({})  {state}\n    {}: {}\n    {}\n    {}",
            channel.id,
            crate::ui::safe(&channel.label),
            channel.kind.name(),
            tr("channels.cli.host", "Address"),
            channel.host,
            view.filters_text.resolve(locale()),
            last,
        )?;
        if view.failing_long {
            writeln!(
                stdout,
                "    ! {}",
                tr(
                    "channels.cli.failing",
                    "Failing for more than a day; check the address or remove the channel."
                )
            )?;
        }
    }
    Ok(0)
}

fn find(id: &str) -> Result<channels::Channel, Error> {
    channels::find(&settings()?, id).ok_or_else(|| {
        localized(cuotai_core::text::Localized::fixed(
            "channels.error.unknown",
            "There is no such channel.",
        ))
    })
}

/// `--channel-remove <id>`.
pub(crate) fn remove(session: &OAuthSession, id: &str) -> Result<u8, Error> {
    let channel = find(id)?;
    channels::remove(
        &settings()?,
        &Vault::new(session.token_store()),
        &channel.id,
    )
    .map_err(localized)?;
    println!(
        "{}",
        trf(
            "channels.cli.removed",
            "Channel {id} removed; its address left the vault.",
            &[("id", &channel.id)]
        )
    );
    Ok(0)
}

/// `--channel-test <id>`: exit 0 when the destination accepted the
/// sample, 2 otherwise.
pub(crate) async fn test(session: &Arc<OAuthSession>, id: &str) -> Result<u8, Error> {
    let channel = find(id)?;
    let dispatcher = dispatcher(session).ok_or_else(|| -> Error {
        tr(
            "channels.cli.unavailable",
            "Channels are unavailable here (no configuration folder or no TLS).",
        )
        .into()
    })?;
    let outcome = dispatcher
        .test(&channel.id, locale(), Utc::now())
        .await
        .map_err(localized)?;
    match outcome {
        Outcome::Delivered { status } => {
            println!(
                "{}",
                trf(
                    "channels.cli.test_ok",
                    "Sent: {host} answered {status}.",
                    &[("host", &channel.host), ("status", &status)]
                )
            );
            Ok(0)
        }
        Outcome::Failed { status, error, .. } => {
            let reason = status
                .map(|s| s.to_string())
                .or(error.map(str::to_owned))
                .unwrap_or_default();
            println!(
                "{}",
                trf(
                    "channels.cli.test_failed",
                    "Not sent: {host} ({reason}).",
                    &[("host", &channel.host), ("reason", &reason)]
                )
            );
            Ok(2)
        }
    }
}

/// `--channel-log [--json]`: the delivery log, never a body or a URL.
pub(crate) fn log(json: bool) -> Result<u8, Error> {
    let entries = DeliveryLog::in_data_dir()
        .map(|log| log.read())
        .unwrap_or_default();
    let mut stdout = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(
            &mut stdout,
            &serde_json::json!({ "schema": 1, "entries": entries }),
        )?;
        writeln!(stdout)?;
        return Ok(0);
    }
    if entries.is_empty() {
        writeln!(
            stdout,
            "{}",
            tr("channels.cli.log_empty", "No delivery in the last 7 days.")
        )?;
        return Ok(0);
    }
    for entry in &entries {
        let outcome = serde_json::to_value(entry.outcome)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let status = entry.status.map(|s| s.to_string()).unwrap_or_default();
        writeln!(
            stdout,
            "{}  {}  {:<16} {:<13} {:>3}  #{}{}",
            crate::ui::local_time(entry.at),
            entry.channel,
            entry.kind,
            outcome,
            status,
            entry.attempt,
            entry
                .error
                .as_deref()
                .map(|e| format!("  {e}"))
                .unwrap_or_default(),
        )?;
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuotai_core::oauth::{OAuthError, OAuthToken, TokenStore};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Memory(Mutex<HashMap<String, OAuthToken>>);

    impl TokenStore for Memory {
        fn load(&self, id: &str) -> Result<Option<OAuthToken>, OAuthError> {
            Ok(self.0.lock().unwrap().get(id).cloned())
        }
        fn save(&self, id: &str, token: &OAuthToken) -> Result<(), OAuthError> {
            self.0.lock().unwrap().insert(id.into(), token.clone());
            Ok(())
        }
        fn delete(&self, id: &str) -> Result<(), OAuthError> {
            self.0.lock().unwrap().remove(id);
            Ok(())
        }
    }

    const URL: &str = "https://hooks.slack.com/services/T0/B0/SECRETTOKEN";

    struct Run {
        result: Result<u8, String>,
        out: String,
        err: String,
        settings: PathBuf,
        store: Arc<Memory>,
        _dir: tempfile::TempDir,
    }

    fn run(input: &str, terminal: bool, yes: bool) -> Run {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("desktop.json");
        let store = Arc::new(Memory::default());
        let vault = Vault::new(store.clone());
        let filters = vec!["kinds=threshold,exhausted".to_owned()];
        let args = AddArgs {
            kind: "slack",
            label: Some("Team"),
            filters: &filters,
            sign: false,
            yes,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let result = add_with(
            &settings,
            &vault,
            &args,
            &mut io::Cursor::new(input.as_bytes()),
            terminal,
            &mut out,
            &mut err,
        )
        .map_err(|e| e.to_string());
        Run {
            result,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
            settings,
            store,
            _dir: dir,
        }
    }

    /// RF5.2: the URL comes from standard input, the consent is printed,
    /// the address is masked and never written to the preferences.
    #[test]
    fn the_url_comes_from_stdin_and_is_masked() {
        let run = run(&format!("{URL}\n"), false, true);
        assert_eq!(run.result, Ok(0));
        assert!(
            run.err.contains("https://hooks.slack.com/••••"),
            "{}",
            run.err
        );
        assert!(run
            .err
            .contains(&channels::consent_sentence().resolve(locale())));
        let saved = std::fs::read_to_string(&run.settings).unwrap();
        for text in [&run.out, &run.err, &saved] {
            assert!(!text.contains("SECRETTOKEN"), "{text}");
        }
        let channel = &channels::read(&run.settings)[0];
        assert_eq!(channel.filters.kinds.len(), 2);
        assert_eq!(run.store.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn saving_needs_a_terminal_confirmation_or_yes() {
        let piped = run(&format!("{URL}\n"), false, false);
        assert!(piped.result.unwrap_err().contains("--yes"));
        assert!(channels::read(&piped.settings).is_empty());
        let refused = run(&format!("{URL}\nn\n"), true, false);
        assert_eq!(
            refused.result.unwrap_err(),
            tr("channels.cli.cancelled", "Nothing was saved.")
        );
        assert!(channels::read(&refused.settings).is_empty());
        let accepted = run(&format!("{URL}\ny\n"), true, false);
        assert_eq!(accepted.result, Ok(0));
        assert!(accepted.err.starts_with(tr(
            "channels.cli.prompt",
            "Paste the address (it stays out of your shell history): "
        )));
        assert_eq!(channels::read(&accepted.settings).len(), 1);
        let wrong = run("https://example.com/x\n", false, true);
        assert!(wrong.result.unwrap_err().contains("hooks.slack.com"));
    }
}
