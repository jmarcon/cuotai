//! PRD 0013 RF3: hooks with fixture scripts run through `/bin/sh` (data
//! arrives as data, the environment is reduced, a timeout kills, output is
//! bounded, at most four run at once, the queue drops the oldest) and the
//! confirmation that guards them.

use super::*;
use crate::events::{MetricRef, Values, SCHEMA};
use crate::MetricWindow;
use chrono::TimeZone;
use std::collections::BTreeSet;

fn event(kind: EventKind, account: &str, threshold: Option<u8>) -> Event {
    Event {
        schema: SCHEMA,
        event: kind,
        occurred_at: Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap(),
        account_key: format!("claude:{account}"),
        account_name: account.into(),
        provider: Provider::Claude,
        metric: Some(MetricRef {
            label: "Sessão (5h)".into(),
            window: Some(MetricWindow::Window5h),
        }),
        current: Values {
            threshold,
            used: Some(91.0),
            limit: Some(100.0),
            ..Values::default()
        },
        previous: None,
        sample: false,
    }
}

fn hooks(text: &str) -> Hooks {
    Hooks::from_toml(PathBuf::from("/nowhere/hooks.toml"), text).unwrap()
}

fn error_id(text: &str) -> String {
    Hooks::from_toml(PathBuf::from("hooks.toml"), text)
        .unwrap_err()
        .id
        .into_owned()
}

#[test]
fn a_rule_reads_with_its_filters_and_defaults() {
    let hooks = hooks(
        r#"
# Comments do not matter.
[[hooks]]
name = "sound"
events = ["quota_exhausted", "quota_threshold_crossed", "quota_exhausted"]
provider = "claude"
account = "Work"
min_threshold = 90
command = ["afplay", "Glass.aiff"]

[[hooks]]
name = "log"
events = ["refresh_failed"]
command = ["logger", "-t", "cuotai"]
timeout_seconds = 60
"#,
    );
    assert_eq!(
        hooks.rules(),
        [
            Rule {
                name: "sound".into(),
                events: vec![EventKind::QuotaExhausted, EventKind::QuotaThresholdCrossed],
                provider: Some(Provider::Claude),
                account: Some("Work".into()),
                min_threshold: Some(90),
                command: vec!["afplay".into(), "Glass.aiff".into()],
                timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            },
            Rule {
                name: "log".into(),
                events: vec![EventKind::RefreshFailed],
                provider: None,
                account: None,
                min_threshold: None,
                command: vec!["logger".into(), "-t".into(), "cuotai".into()],
                timeout_seconds: 60,
            },
        ]
    );
}

/// RF3.1: an argument vector only (no shell string, no relative program),
/// no unknown key (so no secrets or environment ride along), valid names,
/// events, filters and limits.
#[test]
fn invalid_files_are_rejected_with_their_reason() {
    let rule = |extra: &str| {
        format!("[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = [\"true\"]\n{extra}")
    };
    let cases: Vec<(String, &str)> = vec![
        (String::new(), "hooks.error.empty"),
        ("not toml =".into(), "hooks.error.toml"),
        (
            "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = \"true; rm -rf ~\"\n".into(),
            "hooks.error.toml",
        ),
        (rule("env = { TOKEN = \"x\" }"), "hooks.error.toml"),
        (rule("timeout_seconds = 61"), "hooks.error.timeout"),
        (rule("timeout_seconds = 0"), "hooks.error.timeout"),
        (rule("min_threshold = 0"), "hooks.error.min_threshold"),
        (rule("min_threshold = 101"), "hooks.error.min_threshold"),
        (rule("provider = \"nope\""), "hooks.error.provider"),
        (rule("account = \" \""), "hooks.error.account"),
        (format!("{}{}", rule(""), rule("")), "hooks.error.duplicate"),
        (
            "[[hooks]]\nname = \"a\"\nevents = []\ncommand = [\"true\"]\n".into(),
            "hooks.error.no_events",
        ),
        (
            "[[hooks]]\nname = \"a\"\nevents = [\"nope\"]\ncommand = [\"true\"]\n".into(),
            "hooks.error.unknown_event",
        ),
        (
            "[[hooks]]\nname = \"\"\nevents = [\"quota_exhausted\"]\ncommand = [\"true\"]\n".into(),
            "hooks.error.name",
        ),
        (
            "[[hooks]]\nname = \"a\\u001b\"\nevents = [\"quota_exhausted\"]\ncommand = [\"true\"]\n".into(),
            "hooks.error.name",
        ),
    ];
    for (text, id) in &cases {
        assert_eq!(error_id(text), *id, "{text}");
    }
    for command in [
        "[]",
        "[\"\"]",
        "[\"bin/run\"]",
        "[\"./run\"]",
        "[\"..\"]",
        "[\"/bin/x\", \"a\\u0000b\"]",
    ] {
        let text = format!(
            "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = {command}\n"
        );
        assert_eq!(error_id(&text), "hooks.error.command", "{command}");
    }
    let error = Hooks::from_toml(PathBuf::from("hooks.toml"), "\n\n\nname = = 1\n").unwrap_err();
    assert_eq!(error.params["line"], ParamValue::from("4"));
}

#[test]
fn filters_narrow_the_events_a_rule_runs_on() {
    let rule = |extra: &str| {
        hooks(&format!(
            "[[hooks]]\nname = \"a\"\nevents = [\"quota_threshold_crossed\", \"refresh_failed\"]\ncommand = [\"true\"]\n{extra}"
        ))
        .rules()[0]
        .clone()
    };
    let crossed =
        |account, threshold| event(EventKind::QuotaThresholdCrossed, account, Some(threshold));
    let failed = event(EventKind::RefreshFailed, "Work", None);
    assert!(rule("").matches(&crossed("Work", 75)));
    assert!(!rule("").matches(&event(EventKind::QuotaExhausted, "Work", None)));
    assert!(rule("provider = \"claude\"").matches(&failed));
    assert!(!rule("provider = \"groq\"").matches(&failed));
    assert!(rule("account = \"Work\"").matches(&failed));
    assert!(!rule("account = \"Home\"").matches(&failed));
    assert!(!rule("min_threshold = 90").matches(&crossed("Work", 75)));
    assert!(rule("min_threshold = 90").matches(&crossed("Work", 90)));
    // Events without a threshold are not filtered by it.
    assert!(rule("min_threshold = 90").matches(&failed));
}

/// RF3.2: nothing is confirmed at first; a confirmation holds for exactly
/// those rules of that file (layout and comments aside) and a changed
/// command, event or filter needs a new one.
#[test]
fn confirmation_is_mandatory_and_follows_the_rules() {
    let dir = tempfile::tempdir().unwrap();
    let consent = Consent::at(dir.path().join("data/consent.json"));
    let path = dir.path().join("hooks.toml");
    let text =
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = [\"echo\", \"one\"]\n";
    let load = |text: &str| Hooks::from_toml(path.clone(), text).unwrap();

    let unconfirmed = consent.check(load(text)).unwrap_err();
    assert_eq!(
        unconfirmed,
        load(text),
        "the same rules come back to confirm"
    );
    consent.confirm(load(text)).unwrap();
    // A new store over the same file reads the confirmation from disk.
    let again = Consent::at(dir.path().join("data/consent.json"));
    assert!(again.check(load(text)).is_ok());
    assert!(again
        .check(load(&format!("# reformatted\n\n{text}")))
        .is_ok());

    for changed in [
        text.replace("\"one\"", "\"two\""),
        text.replace("quota_exhausted", "quota_reset"),
        format!("{text}account = \"Work\"\n"),
        format!("{text}timeout_seconds = 30\n"),
    ] {
        assert!(again.check(load(&changed)).is_err(), "{changed}");
    }
    let elsewhere = Hooks::from_toml(dir.path().join("other/hooks.toml"), text).unwrap();
    assert!(
        again.check(elsewhere).is_err(),
        "confirmations are per file"
    );

    // Confirming a change replaces the previous confirmation of that file.
    let changed = text.replace("\"one\"", "\"two\"");
    again.confirm(load(&changed)).unwrap();
    assert!(again.check(load(&changed)).is_ok());
    assert!(again.check(load(text)).is_err());
}

#[test]
fn a_damaged_consent_record_confirms_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consent.json");
    std::fs::write(&file, "{not json").unwrap();
    let hooks = Hooks::from_toml(
        dir.path().join("hooks.toml"),
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = [\"true\"]\n",
    )
    .unwrap();
    let consent = Consent::at(&file);
    assert!(consent.check(hooks.clone()).is_err());
    consent.confirm(hooks.clone()).unwrap();
    assert!(consent.check(hooks).is_ok());
}

/// RF3.2: what the user confirms shows every command exactly, quotes and
/// separators included.
#[test]
fn the_listing_shows_each_command_exactly() {
    let hooks = hooks(
        r#"[[hooks]]
name = "notify"
events = ["quota_exhausted", "refresh_failed"]
account = "Work \"main\""
min_threshold = 90
command = ["notify", "it's \"done\"; rm -rf ~", "$(id)"]
"#,
    );
    let lines: Vec<String> = hooks
        .listing()
        .iter()
        .map(|line| line.resolve("en-US"))
        .collect();
    assert_eq!(
        lines,
        [
            "The hooks in /nowhere/hooks.toml run these commands when the listed events happen:",
            "1. notify",
            "events: quota_exhausted, refresh_failed",
            "filters: account = \"Work \\\"main\\\"\", min_threshold = 90",
            "time limit: 10 s",
            r#"command: ["notify","it's \"done\"; rm -rf ~","$(id)"]"#,
        ]
    );
}

#[test]
fn every_message_reads_in_each_catalog() {
    let hooks = hooks(
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\nprovider = \"claude\"\ncommand = [\"true\"]\n",
    );
    let run = |outcome, exit_code| Run {
        rule: "a".into(),
        event: EventKind::QuotaExhausted,
        started_at: Utc::now(),
        duration: Duration::from_millis(12),
        outcome,
        exit_code,
        output: String::new(),
        truncated: false,
    };
    let mut messages = hooks.listing();
    messages.extend([
        confirm_answer(),
        confirm_question("yes"),
        not_confirmed(&hooks),
        declined(),
        dropped(2),
        log_unwritable(Path::new("/x/runs.log")),
        run(Outcome::Exited, Some(0)).summary(),
        run(Outcome::Exited, Some(3)).summary(),
        run(Outcome::Terminated, None).summary(),
        run(Outcome::TimedOut, None).summary(),
        run(Outcome::NotStarted, None).summary(),
    ]);
    let invalid = [
        "",
        "x =",
        "[[hooks]]\nname = \"\"\nevents = [\"quota_exhausted\"]\ncommand = [\"true\"]\n",
        "[[hooks]]\nname = \"a\"\nevents = []\ncommand = [\"true\"]\n",
        "[[hooks]]\nname = \"a\"\nevents = [\"x\"]\ncommand = [\"true\"]\n",
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\nprovider = \"x\"\ncommand = [\"true\"]\n",
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\naccount = \"\"\ncommand = [\"true\"]\n",
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\nmin_threshold = 0\ncommand = [\"true\"]\n",
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = []\n",
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = [\"true\"]\ntimeout_seconds = 0\n",
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = [\"true\"]\n[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = [\"true\"]\n",
    ];
    messages.extend(
        invalid
            .into_iter()
            .map(|text| Hooks::from_toml(PathBuf::from("hooks.toml"), text).unwrap_err()),
    );
    let dir = tempfile::tempdir().unwrap();
    messages.push(Hooks::load(&dir.path().join("hooks.toml")).unwrap_err());
    messages.push(Hooks::load(dir.path()).unwrap_err());
    let blocked = dir.path().join("file");
    std::fs::write(&blocked, "").unwrap();
    messages.push(
        Consent::at(blocked.join("consent.json"))
            .confirm(hooks)
            .unwrap_err(),
    );
    messages.push(changed());
    let valid = vec![RuleDraft {
        name: "a".into(),
        events: vec!["quota_exhausted".into()],
        command: vec!["true".into()],
        ..RuleDraft::default()
    }];
    messages.push(Document::save(&blocked.join("hooks.toml"), None, valid).unwrap_err());
    let ids: BTreeSet<&str> = messages.iter().map(|message| &*message.id).collect();
    let catalog: BTreeSet<&str> = crate::text::core_catalog("en-US")
        .unwrap()
        .keys()
        .map(String::as_str)
        .filter(|id| id.starts_with("hooks."))
        .collect();
    assert_eq!(ids, catalog, "every hooks message is exercised");
    for message in &messages {
        assert!(!message.fallback.contains('{'), "{message:?}");
        for locale in ["en-US", "es", "it"] {
            let text = message.resolve(locale);
            assert!(!text.contains('{'), "{text}");
            // Spanish shares some words with Portuguese ("eventos").
            if locale == "en-US" && message.id != "hooks.confirm.rule" {
                assert_ne!(text, message.fallback, "{} in {locale}", message.id);
            }
        }
    }
}

#[test]
fn the_log_rotates_and_holds_no_payload() {
    let dir = tempfile::tempdir().unwrap();
    let log = RunLog::at(dir.path().join("hooks/runs.log"));
    let run = Run {
        rule: "sound".into(),
        event: EventKind::QuotaExhausted,
        started_at: Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap(),
        duration: Duration::from_millis(1234),
        outcome: Outcome::Exited,
        exit_code: Some(3),
        output: "secret-output-marker".into(),
        truncated: false,
    };
    log.append(&run).unwrap();
    let text = std::fs::read_to_string(log.path()).unwrap();
    assert_eq!(
        text,
        "{\"at\":\"2026-10-20T12:00:00Z\",\"rule\":\"sound\",\"event\":\"quota_exhausted\",\"duration_ms\":1234,\"outcome\":\"exited\",\"exit_code\":3}\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(log.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    std::fs::write(log.path(), vec![b'x'; LOG_ROTATE_BYTES as usize]).unwrap();
    log.append(&run).unwrap();
    assert_eq!(
        std::fs::read_to_string(log.path()).unwrap().lines().count(),
        1
    );
    assert_eq!(
        std::fs::metadata(dir.path().join("hooks/runs.log.1"))
            .unwrap()
            .len(),
        LOG_ROTATE_BYTES
    );
}

/// RF4.1: the desktop's log of the latest runs reads them back newest
/// first, the rotated log included, skipping lines that are not runs.
#[test]
fn the_latest_runs_read_back_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let log = RunLog::at(dir.path().join("runs.log"));
    assert!(log.latest(5).is_empty(), "no log is no run");
    let run = |rule: &str, ms| Run {
        rule: rule.into(),
        event: EventKind::RefreshFailed,
        started_at: Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap(),
        duration: Duration::from_millis(ms),
        outcome: Outcome::Exited,
        exit_code: Some(1),
        output: String::new(),
        truncated: false,
    };
    let older = serde_json::to_string(&run("old", 1).logged()).unwrap();
    std::fs::write(dir.path().join("runs.log.1"), format!("{older}\n")).unwrap();
    log.append(&run("a", 2)).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(log.path())
        .unwrap()
        .write_all(b"{\"truncated\nnot json\n")
        .unwrap();
    log.append(&run("b", 3)).unwrap();
    let rules = |runs: Vec<LoggedRun>| runs.into_iter().map(|run| run.rule).collect::<Vec<_>>();
    assert_eq!(rules(log.latest(10)), ["b", "a", "old"]);
    assert_eq!(rules(log.latest(2)), ["b", "a"]);
    assert_eq!(log.latest(1), [run("b", 3).logged()]);
    assert_eq!(
        log.latest(1)[0].summary().resolve("en-US"),
        "b (refresh_failed): exited with code 1 after 3 ms"
    );
}

/// RF4.1: a rule's sample passes its own filters, is marked as a sample
/// in the payload (real events never are) and exists for every event.
#[test]
fn a_sample_passes_its_rule_and_says_it_is_one() {
    let hooks = hooks(concat!(
        "[[hooks]]\nname = \"filtered\"\n",
        "events = [\"quota_threshold_crossed\", \"refresh_failed\"]\n",
        "provider = \"groq\"\naccount = \"Work\"\nmin_threshold = 95\ncommand = [\"true\"]\n",
        "[[hooks]]\nname = \"plain\"\nevents = [\"provider_incident\"]\ncommand = [\"true\"]\n",
    ));
    let now = Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap();
    let [filtered, plain] = hooks.rules() else {
        panic!("two rules")
    };
    let sample = filtered.sample(now);
    assert!(filtered.matches(&sample));
    assert_eq!(
        (
            sample.event,
            sample.provider,
            sample.account_name.as_str(),
            sample.current.threshold,
        ),
        (
            EventKind::QuotaThresholdCrossed,
            Provider::Groq,
            "Work",
            Some(95)
        )
    );
    assert_eq!(serde_json::to_value(&sample).unwrap()["sample"], true);
    let real = serde_json::to_value(event(EventKind::QuotaExhausted, "Work", None)).unwrap();
    assert!(real.get("sample").is_none(), "{real}");

    let incident = plain.sample(now);
    assert!(plain.matches(&incident));
    assert_eq!(
        (incident.account_name.as_str(), &incident.metric),
        (SAMPLE_ACCOUNT, &None)
    );
    for kind in EventKind::ALL {
        let rule = Rule {
            events: vec![kind],
            ..plain.clone()
        };
        let sample = rule.sample(now);
        assert!(rule.matches(&sample) && sample.event == kind && sample.sample);
        assert!(!sample.summary().resolve("en-US").contains('{'), "{kind:?}");
    }
}

fn draft(name: &str, command: &[&str]) -> RuleDraft {
    RuleDraft {
        name: name.into(),
        events: vec!["quota_exhausted".into()],
        command: command.iter().map(|arg| (*arg).to_owned()).collect(),
        ..RuleDraft::default()
    }
}

/// RF4.1: the editor reads a missing file as an empty one, saves exactly
/// the commands it was given, never over a file edited since it was read,
/// and never writes rules the file would reject.
#[test]
fn the_editor_saves_only_over_the_revision_it_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hooks.toml");
    let missing = Document::read(&path);
    assert_eq!(
        (missing.revision(), missing.drafts()),
        (None, Some(&[][..]))
    );
    assert_eq!(missing.hooks().unwrap_err().id, "hooks.error.missing");

    let saved = Document::save(
        &path,
        None,
        vec![draft("sound", &["afplay", "it's \"x\"; $(id)"])],
    )
    .unwrap();
    assert_eq!(
        saved.hooks().unwrap().rules()[0].command,
        ["afplay", "it's \"x\"; $(id)"]
    );
    assert_eq!(saved, Document::read(&path));
    assert!(saved.revision().is_some());

    let edited = format!("# mine\n{}", std::fs::read_to_string(&path).unwrap());
    std::fs::write(&path, &edited).unwrap();
    for stale in [saved.revision(), None] {
        let error = Document::save(&path, stale, vec![draft("x", &["true"])]).unwrap_err();
        assert_eq!(error.id, "hooks.error.changed");
    }
    let current = Document::read(&path);
    for (drafts, id) in [
        (vec![draft("bad", &["bin/run"])], "hooks.error.command"),
        (vec![], "hooks.error.empty"),
        (
            vec![draft("a", &["true"]), draft("a", &["true"])],
            "hooks.error.duplicate",
        ),
    ] {
        let error = Document::save(&path, current.revision(), drafts).unwrap_err();
        assert_eq!(error.id, id);
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);

    std::fs::write(&path, "not toml =").unwrap();
    let broken = Document::read(&path);
    assert_eq!(broken.drafts(), None);
    assert_eq!(broken.hooks().unwrap_err().id, "hooks.error.toml");
}

/// RF4.1: an edit keeps the comments and filters of the rules it keeps;
/// a removed rule takes its own comments with it.
#[test]
fn saving_keeps_the_comments_of_the_rules_it_keeps() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hooks.toml");
    let text = concat!(
        "# My hooks.\n\n# Plays a sound.\n[[hooks]]\nname = \"sound\"\n",
        "events = [\"quota_exhausted\"] # when it runs out\nprovider = \"claude\"\n",
        "command = [\"afplay\", \"Glass.aiff\"]\n\n",
        "# Gone soon.\n[[hooks]]\nname = \"old\"\nevents = [\"refresh_failed\"]\ncommand = [\"true\"]\n",
    );
    std::fs::write(&path, text).unwrap();
    let read = Document::read(&path);
    let mut drafts = read.drafts().unwrap().to_vec();
    assert_eq!(drafts[0].provider.as_deref(), Some("claude"));
    drafts[0].events.push("quota_reset".into());
    drafts.remove(1);
    drafts.push(draft("new", &["logger", "-t", "cuotai"]));
    Document::save(&path, read.revision(), drafts.clone()).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(
        written.starts_with("# My hooks.\n\n# Plays a sound.\n[[hooks]]\nname = \"sound\"\n"),
        "{written}"
    );
    assert!(written.contains("# when it runs out"), "{written}");
    assert!(written.contains("provider = \"claude\""), "{written}");
    assert!(
        !written.contains("Gone soon") && !written.contains("\"old\""),
        "{written}"
    );
    assert_eq!(Document::read(&path).drafts().unwrap(), drafts.as_slice());
}

#[cfg(unix)]
mod running {
    use super::*;
    use tokio::sync::mpsc;

    /// A sandbox with fixture scripts, its consent and its log.
    struct Rig {
        dir: tempfile::TempDir,
    }

    impl Rig {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().unwrap(),
            }
        }

        fn path(&self, name: &str) -> PathBuf {
            self.dir.path().join(name)
        }

        /// `script` as a file, run as `/bin/sh <file> <args>` so no test
        /// executes a file it just wrote.
        fn script(&self, name: &str, script: &str) -> String {
            let path = self.path(name);
            std::fs::write(&path, script).unwrap();
            path.display().to_string()
        }

        /// One confirmed rule per `(name, events, command)`.
        fn confirmed(&self, rules: &[(&str, &str, Vec<String>, u64)]) -> Confirmed {
            let text: String = rules
                .iter()
                .map(|(name, events, command, timeout)| {
                    format!(
                        "[[hooks]]\nname = \"{name}\"\nevents = [{events}]\ncommand = {}\ntimeout_seconds = {timeout}\n",
                        serde_json::to_string(command).unwrap()
                    )
                })
                .collect();
            let hooks = Hooks::from_toml(self.path("hooks.toml"), &text).unwrap();
            Consent::at(self.path("consent.json"))
                .confirm(hooks)
                .unwrap()
        }

        fn start(
            &self,
            confirmed: Confirmed,
            parent: Vec<(&str, &str)>,
        ) -> (Runner, mpsc::UnboundedReceiver<Run>) {
            let (sender, receiver) = mpsc::unbounded_channel();
            let on_run: OnRun = Arc::new(move |run: &Run, logged: Option<&io::Error>| {
                assert!(logged.is_none(), "{logged:?}");
                let _ = sender.send(run.clone());
            });
            let parent = parent
                .into_iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value)));
            let runner = Runner::start_with_env(
                confirmed,
                Some(RunLog::at(self.path("runs.log"))),
                on_run,
                parent,
                Some(self.path("data").join("hooks").join("work")),
            );
            (runner, receiver)
        }
    }

    /// Wait until `path` holds a complete line (a pid a script wrote).
    async fn read_pid(path: &Path) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(pid) = std::fs::read_to_string(path)
                .ok()
                .filter(|pid| pid.ends_with('\n'))
            {
                return pid;
            }
            assert!(Instant::now() < deadline, "the pid never arrived");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Wait until `pid` is gone, failing after a few seconds.
    async fn gone(pid: &str, what: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while alive(pid) {
            assert!(Instant::now() < deadline, "{what}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn sh(script: &str, args: &[&str]) -> Vec<String> {
        ["/bin/sh", script]
            .into_iter()
            .chain(args.iter().copied())
            .map(String::from)
            .collect()
    }

    /// Whether `pid` still runs (a zombie waiting to be reaped is dead).
    fn alive(pid: &str) -> bool {
        let out = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", pid.trim()])
            .output()
            .unwrap();
        let stat = String::from_utf8_lossy(&out.stdout);
        let stat = stat.trim();
        !stat.is_empty() && !stat.starts_with('Z')
    }

    async fn next(runs: &mut mpsc::UnboundedReceiver<Run>) -> Run {
        tokio::time::timeout(Duration::from_secs(20), runs.recv())
            .await
            .expect("a run in time")
            .unwrap()
    }

    /// RF3.3: arguments with quotes, `;`, `$(...)` and globs reach the
    /// program as they are, the event arrives as JSON on stdin and in the
    /// `CUOTAI_*` variables (an account name built to break a shell stays
    /// data), the environment holds only the allowed variables (no
    /// credential) and the working directory is the temporary one.
    #[tokio::test]
    async fn the_event_and_the_arguments_arrive_as_data_in_a_reduced_environment() {
        let rig = Rig::new();
        let out = rig.path("out");
        let pwned = rig.path("pwned");
        let script = rig.script(
            "dump.sh",
            &format!(
                r#"out='{out}'
for arg in "$@"; do printf 'arg=%s\n' "$arg" >> "$out/args"; done
cat > "$out/stdin"
env > "$out/env"
pwd -P > "$out/pwd"
"#,
                out = out.display()
            ),
        );
        std::fs::create_dir(&out).unwrap();
        let tricky = format!("it's \"quoted\"; touch {}", pwned.display());
        let substitution = format!("$(touch {})", pwned.display());
        let confirmed = rig.confirmed(&[(
            "dump",
            "\"quota_exhausted\"",
            sh(&script, &[&tricky, &substitution, "*", "a b"]),
            10,
        )]);
        let (runner, mut runs) = rig.start(
            confirmed,
            vec![
                ("PATH", "/usr/bin:/bin"),
                ("HOME", "/home/someone"),
                ("LANG", "en_US.UTF-8"),
                ("AWS_SECRET_ACCESS_KEY", "aws-secret-marker"),
                ("OPENAI_ADMIN_KEY", "sk-admin-marker"),
                ("CUOTAI_CONFIG", "/etc/cuotai.toml"),
                ("LC_ALL", "C"),
            ],
        );
        let account = format!("Work\"; touch {}; echo \"", pwned.display());
        let event = event(EventKind::QuotaExhausted, &account, None);
        assert_eq!(runner.submit(&event), 0);
        let run = next(&mut runs).await;
        assert!(run.succeeded(), "{run:?}");

        let args = std::fs::read_to_string(out.join("args")).unwrap();
        assert_eq!(
            args,
            format!("arg={tricky}\narg={substitution}\narg=*\narg=a b\n")
        );
        let stdin: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(out.join("stdin")).unwrap()).unwrap();
        assert_eq!(stdin, serde_json::to_value(&event).unwrap());
        assert!(!pwned.exists(), "nothing was interpreted by a shell");

        let env = std::fs::read_to_string(out.join("env")).unwrap();
        let vars: BTreeMap<&str, &str> = env
            .lines()
            .filter_map(|line| line.split_once('='))
            .collect();
        assert_eq!(vars["PATH"], "/usr/bin:/bin");
        assert_eq!(vars["HOME"], "/home/someone");
        assert_eq!(vars["LANG"], "en_US.UTF-8");
        assert_eq!(vars["CUOTAI_EVENT"], "quota_exhausted");
        assert_eq!(vars["CUOTAI_ACCOUNT"], account);
        assert_eq!(vars["CUOTAI_PROVIDER"], "claude");
        assert_eq!(vars["CUOTAI_METRIC"], "Sessão (5h)");
        // What `sh` itself adds (PWD, SHLVL, `_`) is no inheritance.
        let shell_own = ["PWD", "OLDPWD", "SHLVL", "_", "__CF_USER_TEXT_ENCODING"];
        let unexpected: Vec<&&str> = vars
            .keys()
            .filter(|name| {
                !INHERITED_ENV.contains(name)
                    && !name.starts_with("CUOTAI_")
                    && !shell_own.contains(name)
            })
            .collect();
        assert!(unexpected.is_empty(), "{unexpected:?}");
        for leaked in ["aws-secret-marker", "sk-admin-marker", "/etc/cuotai.toml"] {
            assert!(!env.contains(leaked), "{leaked}");
        }
        // FIX-02: a private working directory, never the shared temporary
        // one.
        let pwd = std::fs::read_to_string(out.join("pwd")).unwrap();
        let work = rig.path("data").join("hooks").join("work");
        assert_eq!(Path::new(pwd.trim()), std::fs::canonicalize(&work).unwrap());
        assert_ne!(
            Path::new(pwd.trim()),
            std::fs::canonicalize(std::env::temp_dir()).unwrap()
        );
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&work).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }

        let log = std::fs::read_to_string(rig.path("runs.log")).unwrap();
        assert_eq!(log.lines().count(), 1);
        assert!(log.contains("\"rule\":\"dump\""), "{log}");
        assert!(log.contains("\"exit_code\":0"), "{log}");
        assert!(!log.contains("Work"), "the log has no payload: {log}");
    }

    /// RF3.4: a hook past its time limit is killed; output past 16 KiB is
    /// cut; failing and missing programs are runs like any other, and the
    /// runner goes on.
    #[tokio::test]
    async fn timeouts_kill_output_is_bounded_and_failures_stay_local() {
        let rig = Rig::new();
        let pid = rig.path("pid");
        let sleeper = rig.script(
            "sleep.sh",
            &format!("echo $$ > '{}'\nexec sleep 30\n", pid.display()),
        );
        let chatty = rig.script("chatty.sh", "head -c 40000 /dev/zero | tr '\\0' a\n");
        let failing = rig.script("fail.sh", "echo 'oops' >&2\nexit 3\n");
        let confirmed = rig.confirmed(&[
            ("sleep", "\"quota_exhausted\"", sh(&sleeper, &[]), 1),
            ("chatty", "\"quota_reset\"", sh(&chatty, &[]), 10),
            ("fail", "\"refresh_failed\"", sh(&failing, &[]), 10),
            (
                "missing",
                "\"auth_required\"",
                vec![rig.path("absent").display().to_string()],
                10,
            ),
            (
                "after",
                "\"usage_updated\"",
                vec!["/bin/sh".into(), "-c".into(), "exit 0".into()],
                10,
            ),
        ]);
        let (runner, mut runs) = rig.start(confirmed, vec![("PATH", "/usr/bin:/bin")]);

        runner.submit(&event(EventKind::QuotaExhausted, "Work", None));
        let run = next(&mut runs).await;
        assert_eq!(
            (run.rule.as_str(), run.outcome),
            ("sleep", Outcome::TimedOut)
        );
        assert_eq!(run.exit_code, None);
        assert!(run.duration < Duration::from_secs(5), "{run:?}");
        let pid = std::fs::read_to_string(&pid).unwrap();
        assert!(!alive(&pid), "the timed-out process was killed");

        runner.submit(&event(EventKind::QuotaReset, "Work", None));
        let run = next(&mut runs).await;
        assert!(run.succeeded(), "{run:?}");
        assert_eq!(run.output.len(), OUTPUT_LIMIT);
        assert!(run.truncated);

        runner.submit(&event(EventKind::RefreshFailed, "Work", None));
        let run = next(&mut runs).await;
        assert_eq!((run.outcome, run.exit_code), (Outcome::Exited, Some(3)));
        assert_eq!(run.output, "oops\n");
        assert!(!run.truncated);

        runner.submit(&event(EventKind::AuthRequired, "Work", None));
        assert_eq!(next(&mut runs).await.outcome, Outcome::NotStarted);

        runner.submit(&event(EventKind::UsageUpdated, "Work", None));
        assert!(next(&mut runs).await.succeeded(), "the runner goes on");

        let log = std::fs::read_to_string(rig.path("runs.log")).unwrap();
        let outcomes: Vec<serde_json::Value> = log
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap()["outcome"].clone())
            .collect();
        assert_eq!(
            outcomes,
            ["timed_out", "exited", "exited", "not_started", "exited"]
        );
    }

    /// RF3.4: at most four hooks at once; submitting never waits, and past
    /// the queue's capacity the oldest pending runs are dropped and counted.
    #[tokio::test]
    async fn four_at_once_and_a_full_queue_drops_the_oldest() {
        let rig = Rig::new();
        let running = rig.path("running");
        let seen = rig.path("seen");
        std::fs::create_dir(&running).unwrap();
        let script = rig.script(
            "busy.sh",
            &format!(
                r#"touch '{running}/'$$
ls '{running}' | wc -l >> '{seen}.count'
printf '%s\n' "$CUOTAI_ACCOUNT" >> '{seen}'
sleep 0.1
rm '{running}/'$$
"#,
                running = running.display(),
                seen = seen.display()
            ),
        );
        let confirmed = rig.confirmed(&[("busy", "\"quota_exhausted\"", sh(&script, &[]), 10)]);
        let (runner, mut runs) = rig.start(confirmed, vec![("PATH", "/usr/bin:/bin")]);
        // A current-thread runtime: nothing starts before this loop yields.
        let total = QUEUE_CAPACITY + 2;
        let dropped: usize = (0..total)
            .map(|index| {
                runner.submit(&event(
                    EventKind::QuotaExhausted,
                    &format!("n{index}"),
                    None,
                ))
            })
            .sum();
        assert_eq!(dropped, 2);
        for _ in 0..QUEUE_CAPACITY {
            assert!(next(&mut runs).await.succeeded());
        }
        let names: BTreeSet<String> = std::fs::read_to_string(&seen)
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        let expected: BTreeSet<String> = (2..total).map(|index| format!("n{index}")).collect();
        assert_eq!(names, expected, "the two oldest were dropped");
        let counts: Vec<usize> = std::fs::read_to_string(rig.path("seen.count"))
            .unwrap()
            .lines()
            .map(|line| line.trim().parse().unwrap())
            .collect();
        let most = counts.iter().copied().max().unwrap();
        assert!((2..=MAX_RUNNING).contains(&most), "{counts:?}");
    }

    /// Dropping the runner stops the hooks still running.
    #[tokio::test]
    async fn dropping_the_runner_kills_running_hooks() {
        let rig = Rig::new();
        let pid = rig.path("pid");
        let script = rig.script(
            "sleep.sh",
            &format!("echo $$ > '{}'\nexec sleep 30\n", pid.display()),
        );
        let confirmed = rig.confirmed(&[("sleep", "\"quota_exhausted\"", sh(&script, &[]), 60)]);
        let (runner, _runs) = rig.start(confirmed, vec![("PATH", "/usr/bin:/bin")]);
        runner.submit(&event(EventKind::QuotaExhausted, "Work", None));
        let deadline = Instant::now() + Duration::from_secs(20);
        let pid = loop {
            if let Some(pid) = std::fs::read_to_string(&pid)
                .ok()
                .filter(|pid| pid.ends_with('\n'))
            {
                break pid;
            }
            assert!(Instant::now() < deadline, "the hook never started");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        drop(runner);
        let deadline = Instant::now() + Duration::from_secs(10);
        while alive(&pid) {
            assert!(Instant::now() < deadline, "the hook outlived the runner");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// FIX-01: the time limit kills the hook's whole process tree, and a
    /// hook that exits leaves nothing running behind it, so no grandchild
    /// escapes the four-at-once limit.
    #[tokio::test]
    async fn the_whole_process_tree_dies_on_timeout_and_on_exit() {
        let rig = Rig::new();
        let hung = rig.path("hung.pid");
        let left = rig.path("left.pid");
        let hanging = rig.script(
            "hang.sh",
            &format!("sleep 30 &\necho $! > '{}'\nwait\n", hung.display()),
        );
        let leaving = rig.script(
            "leave.sh",
            &format!(
                "sleep 30 > /dev/null 2>&1 &\necho $! > '{}'\nexit 0\n",
                left.display()
            ),
        );
        let confirmed = rig.confirmed(&[
            ("hang", "\"quota_exhausted\"", sh(&hanging, &[]), 1),
            ("leave", "\"quota_reset\"", sh(&leaving, &[]), 10),
        ]);
        let (runner, mut runs) = rig.start(confirmed, vec![("PATH", "/usr/bin:/bin")]);

        runner.submit(&event(EventKind::QuotaExhausted, "Work", None));
        let run = next(&mut runs).await;
        assert_eq!(run.outcome, Outcome::TimedOut);
        let grandchild = read_pid(&hung).await;
        gone(&grandchild, "the timed-out hook's grandchild survived").await;

        runner.submit(&event(EventKind::QuotaReset, "Work", None));
        let run = next(&mut runs).await;
        assert!(run.succeeded(), "{run:?}");
        let orphan = read_pid(&left).await;
        gone(&orphan, "the exited hook left a process running").await;
    }

    /// FIX-02: a working directory that is not a private directory (here a
    /// symbolic link) starts no hook at all.
    #[tokio::test]
    async fn a_working_directory_that_is_a_link_starts_nothing() {
        let rig = Rig::new();
        let elsewhere = rig.path("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        let work = rig.path("data").join("hooks").join("work");
        std::fs::create_dir_all(work.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &work).unwrap();
        let marker = rig.path("ran");
        let confirmed = rig.confirmed(&[(
            "touch",
            "\"quota_exhausted\"",
            vec!["/usr/bin/touch".into(), marker.display().to_string()],
            10,
        )]);
        let (runner, mut runs) = rig.start(confirmed, vec![("PATH", "/usr/bin:/bin")]);
        runner.submit(&event(EventKind::QuotaExhausted, "Work", None));
        assert_eq!(next(&mut runs).await.outcome, Outcome::NotStarted);
        assert!(!marker.exists());
    }

    /// RF4.1: the test button runs one rule alone with its sample, marked
    /// as such on stdin, even where the rule's filters would not let a
    /// real event of another account through; other rules stay idle.
    #[tokio::test]
    async fn a_test_runs_only_the_named_rule_with_its_sample() {
        let rig = Rig::new();
        let out = rig.path("stdin");
        let script = rig.script("cat.sh", &format!("cat > '{}'\n", out.display()));
        let other = rig.path("other");
        let touch = rig.script("touch.sh", &format!("touch '{}'\n", other.display()));
        let confirmed = rig.confirmed(&[
            ("tested", "\"refresh_failed\"", sh(&script, &[]), 10),
            ("idle", "\"refresh_failed\"", sh(&touch, &[]), 10),
        ]);
        let (runner, mut runs) = rig.start(confirmed, vec![("PATH", "/usr/bin:/bin")]);
        let sample = runner.rules()[0].sample(Utc::now());
        assert_eq!(runner.submit_to("nope", &sample), None);
        assert_eq!(runner.submit_to("tested", &sample), Some(0));
        let run = next(&mut runs).await;
        assert_eq!(
            (run.rule.as_str(), run.event),
            ("tested", EventKind::RefreshFailed)
        );
        assert!(run.succeeded(), "{run:?}");
        let stdin: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(stdin["sample"], true);
        assert_eq!(stdin, serde_json::to_value(&sample).unwrap());
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!other.exists(), "only the named rule ran");
    }
}
