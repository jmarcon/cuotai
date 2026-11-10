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
        account_id: "acc_test00000000".into(),
        account_name: account.into(),
        provider: Provider::Claude,
        accounts: Vec::new(),
        parent: None,
        metric: Some(MetricRef::new("Sessão (5h)", Some(MetricWindow::Window5h))),
        current: Values {
            threshold,
            used: Some(91.0),
            limit: Some(100.0),
            ..Values::default()
        },
        previous: None,
        sample: false,
        row: None,
    }
}

/// The rules of `text` with no `PATH`, so a bare program never resolves
/// and the listing does not depend on the machine.
fn hooks(text: &str) -> Hooks {
    Hooks::from_toml(PathBuf::from("/nowhere/hooks.toml"), text)
        .unwrap()
        .with_search_path(None)
}

/// PRD 0050 RF2.4: a directory of fixture "programs" (never run here),
/// executable on Unix, to put on a rule's `PATH`.
fn bin_dir(names: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in names {
        write_program(&dir.path().join(name), name.as_bytes());
    }
    dir
}

fn write_program(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn on(dirs: &[&Path]) -> Option<OsString> {
    Some(std::env::join_paths(dirs).unwrap())
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
                account_id: None,
                min_threshold: Some(90),
                command: vec!["afplay".into(), "Glass.aiff".into()],
                timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
                quiet: QuietMode::Run,
            },
            Rule {
                name: "log".into(),
                events: vec![EventKind::RefreshFailed],
                provider: None,
                account: None,
                account_id: None,
                min_threshold: None,
                command: vec!["logger".into(), "-t".into(), "cuotai".into()],
                timeout_seconds: 60,
                quiet: QuietMode::Run,
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
        (rule("account_id = \"Not An Id\""), "hooks.error.account_id"),
        (rule("timeout_seconds = 0"), "hooks.error.timeout"),
        (rule("min_threshold = 0"), "hooks.error.min_threshold"),
        (rule("min_threshold = 101"), "hooks.error.min_threshold"),
        (rule("quiet = \"later\""), "hooks.error.quiet"),
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
    let bin = bin_dir(&["echo"]);
    let text =
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = [\"echo\", \"one\"]\n";
    let load = |text: &str| {
        Hooks::from_toml(path.clone(), text)
            .unwrap()
            .with_search_path(on(&[bin.path()]))
    };

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
    let elsewhere = Hooks::from_toml(dir.path().join("other/hooks.toml"), text)
        .unwrap()
        .with_search_path(on(&[bin.path()]));
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
    let bin = bin_dir(&["true"]);
    let hooks = Hooks::from_toml(
        dir.path().join("hooks.toml"),
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = [\"true\"]\n",
    )
    .unwrap()
    .with_search_path(on(&[bin.path()]));
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
            "program: not found on PATH (it cannot be approved)",
        ]
    );
}

/// PRD 0050 RF2.4: a bare program is the first executable regular file of
/// that name on the captured `PATH` (relative entries skipped), pinned by
/// its canonical path and SHA-256; an absolute one is itself; neither
/// found is unresolved.
#[test]
fn a_bare_program_is_pinned_on_the_captured_path() {
    use sha2::{Digest, Sha256};
    let first = bin_dir(&["tool"]);
    let second = bin_dir(&["tool", "other"]);
    let search = on(&[Path::new("relative"), first.path(), second.path()]);
    let pin = pin::resolve("tool", search.as_deref()).unwrap();
    let expected = std::fs::canonicalize(first.path().join("tool")).unwrap();
    assert_eq!(pin.program, expected);
    assert_eq!(pin.sha256, format!("{:x}", Sha256::digest(b"tool")));
    assert_eq!(pin.interpreter, None);
    assert_eq!(
        pin::resolve("other", search.as_deref()).unwrap().program,
        std::fs::canonicalize(second.path().join("other")).unwrap()
    );
    let absolute = second.path().join("other").display().to_string();
    assert_eq!(
        pin::resolve(&absolute, None).unwrap().program,
        std::fs::canonicalize(&absolute).unwrap()
    );
    assert_eq!(
        pin::resolve("absent", search.as_deref()),
        Err(Unresolved::Program)
    );
    assert_eq!(pin::resolve("tool", None), Err(Unresolved::Program));
    assert_eq!(
        pin::resolve(&second.path().join("absent").display().to_string(), None),
        Err(Unresolved::Program)
    );
    assert_eq!(
        pin::resolve(&first.path().display().to_string(), None),
        Err(Unresolved::Program),
        "a directory is no program"
    );
    #[cfg(unix)]
    {
        // Not executable: skipped, as a shell would.
        use std::os::unix::fs::PermissionsExt;
        let plain = first.path().join("tool");
        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            pin::resolve("tool", search.as_deref()).unwrap().program,
            std::fs::canonicalize(second.path().join("tool")).unwrap()
        );
    }
}

/// PRD 0050 RF2.4: a script's `#!` interpreter is pinned too: an absolute
/// one as itself, `env NAME` as NAME on the same `PATH`; one that cannot
/// be found leaves the program unresolved. Unix only: Windows never runs a
/// `#!` line, so it pins no interpreter (see the test below).
#[cfg(unix)]
#[test]
fn a_script_pins_its_interpreter() {
    let bin = bin_dir(&["interp"]);
    let interp = std::fs::canonicalize(bin.path().join("interp")).unwrap();
    let search = on(&[bin.path()]);
    let scripts = tempfile::tempdir().unwrap();
    let script = |name: &str, line: &str| {
        let path = scripts.path().join(name);
        write_program(&path, format!("{line}\necho hi\n").as_bytes());
        path.display().to_string()
    };
    let direct = script("direct", &format!("#!{} -x", interp.display()));
    let via_env = script("via-env", "#!/usr/bin/env -S interp -u");
    let missing = script("missing", "#!/usr/bin/env not-there");
    let relative = script("relative", "#! interp");

    let pinned = pin::resolve(&direct, search.as_deref()).unwrap();
    let interpreter = pinned.interpreter.unwrap();
    assert_eq!(interpreter.program, interp);
    assert_eq!(
        Some(interpreter),
        pin::resolve(&via_env, search.as_deref())
            .unwrap()
            .interpreter
    );
    assert_eq!(
        pin::resolve(&missing, search.as_deref()),
        Err(Unresolved::Interpreter)
    );
    assert_eq!(
        pin::resolve(&relative, search.as_deref()),
        Err(Unresolved::Shebang)
    );
}

/// PRD 0050 RF2.4 on Windows: a `#!` line is never run there, so a script
/// pins only itself.
#[cfg(windows)]
#[test]
fn windows_pins_no_interpreter() {
    let bin = bin_dir(&["interp"]);
    let scripts = tempfile::tempdir().unwrap();
    let path = scripts.path().join("script");
    write_program(&path, b"#!/usr/bin/env interp\necho hi\n");
    let pinned = pin::resolve(&path.display().to_string(), on(&[bin.path()]).as_deref()).unwrap();
    assert_eq!(pinned.interpreter, None);
}

/// PRD 0050 RF2.4: the listing names each pinned program and interpreter
/// with its hash; the consent digest follows their bytes and which file
/// `PATH` finds, so a confirmation stops matching (and asks again) when
/// either changes; an unresolved program cannot be confirmed at all.
// security-invariant: SEC-HOOK-PIN
#[test]
fn the_digest_covers_the_resolved_program_and_its_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let consent = Consent::at(dir.path().join("consent.json"));
    let early = tempfile::tempdir().unwrap();
    let bin = bin_dir(&["tool", "interp"]);
    let interp = std::fs::canonicalize(bin.path().join("interp")).unwrap();
    write_program(
        &bin.path().join("tool"),
        format!("#!{}\nrun\n", interp.display()).as_bytes(),
    );
    let text = "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = [\"tool\"]\n";
    let load = || {
        Hooks::from_toml(dir.path().join("hooks.toml"), text)
            .unwrap()
            .with_search_path(on(&[early.path(), bin.path()]))
    };

    let hooks = load();
    let pins = hooks.pinned().unwrap();
    let lines: Vec<String> = hooks
        .listing()
        .iter()
        .map(|line| line.resolve("en-US"))
        .collect();
    let tool = std::fs::canonicalize(bin.path().join("tool")).unwrap();
    assert!(
        lines.contains(&format!(
            "program: {} (SHA-256 {})",
            tool.display(),
            pins[0].sha256
        )),
        "{lines:?}"
    );
    #[cfg(unix)]
    assert!(
        lines.contains(&format!(
            "interpreter: {} (SHA-256 {})",
            interp.display(),
            pins[0].interpreter.as_ref().unwrap().sha256
        )),
        "{lines:?}"
    );
    let (shown, digest) = hooks.approval().unwrap();
    assert_eq!(digest, hooks.digest());
    assert!(shown.starts_with(&hooks.listing()));
    assert!(
        consent.check(load()).is_err(),
        "a confirmation of the rules alone (before pinning) asks again"
    );
    consent.confirm_shown(load(), &digest).unwrap();
    let confirmed = consent.check(load()).unwrap();
    assert_eq!(confirmed.pins(), pins.as_slice());

    // New bytes in the interpreter (Unix: Windows pins no interpreter);
    // elsewhere, new bytes in the program itself.
    let changed_file = if cfg!(unix) {
        interp.clone()
    } else {
        tool.clone()
    };
    let original = std::fs::read(&changed_file).unwrap();
    write_program(&changed_file, b"other bytes");
    assert_ne!(load().digest(), digest);
    assert!(consent.check(load()).is_err());
    assert_eq!(
        consent.confirm_shown(load(), &digest).unwrap_err(),
        changed(),
        "what was shown is not what would be approved"
    );
    write_program(&changed_file, &original);
    assert!(consent.check(load()).is_ok(), "the same bytes again");

    // Another file of that name earlier on PATH.
    write_program(&early.path().join("tool"), b"impostor");
    assert_ne!(load().digest(), digest);
    assert!(consent.check(load()).is_err());
    std::fs::remove_file(early.path().join("tool")).unwrap();

    // The program disappears: nothing can be confirmed.
    std::fs::remove_file(bin.path().join("tool")).unwrap();
    assert!(consent.check(load()).is_err());
    assert_eq!(
        consent.confirm(load()).unwrap_err().id,
        "hooks.error.unresolved"
    );
    assert_eq!(load().approval().unwrap_err(), unresolved("a"));
    assert_eq!(
        load().rule_approval("a").unwrap().unwrap_err(),
        unresolved("a")
    );
}

/// PRD 0032 RF5: `quiet = "skip"` is shown in the confirmation listing and
/// changes the confirmation digest; the default `run` keeps the digest of
/// rules written before it existed.
#[test]
fn quiet_is_confirmed_and_the_default_keeps_the_old_digest() {
    let base = "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = [\"true\"]\n";
    let plain = hooks(base);
    let explicit_run = hooks(&format!("{base}quiet = \"run\"\n"));
    let skip = hooks(&format!("{base}quiet = \"skip\"\n"));
    assert_eq!(plain.digest(), explicit_run.digest());
    assert_ne!(plain.digest(), skip.digest(), "the consent digest changes");
    assert_eq!(skip.rules()[0].quiet, QuietMode::Skip);
    let listing: Vec<String> = skip
        .listing()
        .iter()
        .map(|line| line.resolve("en-US"))
        .collect();
    assert!(
        listing.contains(&"filters: quiet = \"skip\"".to_owned()),
        "{listing:?}"
    );
    assert!(!plain
        .listing()
        .iter()
        .any(|line| line.resolve("en-US").starts_with("filters:")));
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
    // PRD 0050 RF2.2: the native review adds the working directory and
    // the environment names to the listing.
    let mut messages = hooks.review();
    assert!(messages.starts_with(&hooks.listing()));
    assert_eq!(hooks.rule_review("a"), Some(hooks.review()));
    assert_eq!(hooks.rule_review("b"), None);
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
        run(Outcome::Changed, None).summary(),
        run(Outcome::Exposed, None).summary(),
        unresolved("a"),
        refused("a", Unresolved::Shebang),
        refused("a", Unresolved::Exposed),
        refused("a", Unresolved::Disguised),
        exposed_file("/x/hooks.toml"),
    ]);
    messages.extend(pin_lines(&Err(Unresolved::Exposed)));
    messages.extend(pin_lines(&Err(Unresolved::Shebang)));
    messages.extend(pin_lines(&Err(Unresolved::Disguised)));
    // PRD 0050 RF2.4: the lines of a pinned program and interpreter.
    let bin = bin_dir(&["interp"]);
    let interp = std::fs::canonicalize(bin.path().join("interp")).unwrap();
    write_program(
        &bin.path().join("tool"),
        format!("#!{}\n", interp.display()).as_bytes(),
    );
    let resolved = Hooks::from_toml(
        PathBuf::from("/nowhere/hooks.toml"),
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = [\"tool\"]\n",
    )
    .unwrap()
    .with_search_path(on(&[bin.path()]));
    messages.extend(resolved.approval().unwrap().0);
    // Windows pins no interpreter (it never runs a `#!` line); its line is
    // still in every catalog for Unix.
    #[cfg(not(unix))]
    messages.extend(pin_lines(&Ok(pin::Pin {
        program: interp.clone(),
        sha256: "0".repeat(64),
        interpreter: Some(pin::Interpreter {
            program: interp.clone(),
            sha256: "0".repeat(64),
        }),
    })));
    let invalid = [
        "",
        "x =",
        "[[hooks]]\nname = \"\"\nevents = [\"quota_exhausted\"]\ncommand = [\"true\"]\n",
        "[[hooks]]\nname = \"a\"\nevents = []\ncommand = [\"true\"]\n",
        "[[hooks]]\nname = \"a\"\nevents = [\"x\"]\ncommand = [\"true\"]\n",
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\nprovider = \"x\"\ncommand = [\"true\"]\n",
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\naccount = \"\"\ncommand = [\"true\"]\n",
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\naccount_id = \"X\"\ncommand = [\"true\"]\n",
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\nmin_threshold = 0\ncommand = [\"true\"]\n",
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = [\"true\"]\nquiet = \"x\"\n",
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
            .confirm(resolved)
            .unwrap_err(),
    );
    messages.push(changed());
    messages.push(claims_unavailable());
    let valid = vec![RuleDraft {
        name: "a".into(),
        events: vec!["quota_exhausted".into()],
        command: vec!["true".into()],
        ..RuleDraft::default()
    }];
    messages.push(Document::save(&blocked.join("hooks.toml"), None, valid).unwrap_err());
    let ids: BTreeSet<&str> = messages.iter().map(|message| &*message.id).collect();
    let catalog: BTreeSet<&str> = crate::text::core_catalog("pt-BR")
        .unwrap()
        .keys()
        .map(String::as_str)
        .filter(|id| id.starts_with("hooks."))
        .collect();
    assert_eq!(ids, catalog, "every hooks message is exercised");
    for message in &messages {
        assert!(!message.fallback.contains('{'), "{message:?}");
        for locale in crate::text::catalog_locales() {
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

/// PRD 0013 S-01: a program, a script's interpreter or `hooks.toml` that
/// another user could change (itself or a folder above it) is never
/// pinned, so it can be neither approved nor listed as approvable; a
/// sticky folder (as `/tmp`) holding the user's own file is fine; an
/// interpreter that is itself a `#!` script and the `env` options that
/// change what runs are refused.
#[cfg(unix)]
#[test]
fn an_exposed_program_interpreter_or_rules_file_is_never_approved() {
    use std::os::unix::fs::PermissionsExt;
    let set = |path: &Path, mode: u32| {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    write_program(&bin.join("interp"), b"interp");
    write_program(&bin.join("tool"), b"tool");
    let interp = std::fs::canonicalize(bin.join("interp")).unwrap();
    let scripts = root.path().join("scripts");
    std::fs::create_dir(&scripts).unwrap();
    let script = scripts.join("hook");
    write_program(&script, format!("#!{}\n", interp.display()).as_bytes());
    let search = on(&[&bin]);
    let rules = |program: &str| {
        Hooks::from_toml(
            root.path().join("hooks.toml"),
            &format!("[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = [{program:?}]\n"),
        )
        .unwrap()
        .with_search_path(search.clone())
    };
    let script_path = script.display().to_string();
    assert!(rules("tool").approval().is_ok());
    assert!(rules(&script_path).approval().is_ok());

    // The program itself, then a folder above it.
    set(&bin.join("tool"), 0o757);
    assert_eq!(rules("tool").pins()[0], Err(Unresolved::Exposed));
    assert_eq!(
        rules("tool").approval().unwrap_err().id,
        "hooks.error.exposed"
    );
    assert_eq!(
        rules("tool").listing().last().unwrap().id,
        "hooks.confirm.exposed"
    );
    set(&bin.join("tool"), 0o755);
    set(&bin, 0o777);
    assert_eq!(rules("tool").pins()[0], Err(Unresolved::Exposed));
    // The interpreter's folder exposes the script.
    assert_eq!(rules(&script_path).pins()[0], Err(Unresolved::Exposed));
    set(&bin, 0o755);
    // A sticky folder holding the user's own script is fine.
    set(&scripts, 0o1777);
    assert!(rules(&script_path).approval().is_ok(), "sticky");
    set(&scripts, 0o755);

    // A nested interpreter, and `env -P`, cannot be pinned.
    write_program(
        &bin.join("nested"),
        format!("#!{}\n", interp.display()).as_bytes(),
    );
    write_program(
        &script,
        format!("#!{}\n", bin.join("nested").display()).as_bytes(),
    );
    assert_eq!(
        rules(&script_path).approval().unwrap_err().id,
        "hooks.error.shebang"
    );
    write_program(&script, b"#!/usr/bin/env -P /tmp interp\n");
    assert_eq!(rules(&script_path).pins()[0], Err(Unresolved::Shebang));

    // hooks.toml (or its folder) writable by others is refused on load.
    let file = root.path().join("hooks.toml");
    std::fs::write(
        &file,
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = [\"tool\"]\n",
    )
    .unwrap();
    set(&file, 0o644);
    assert!(Hooks::load(&file).is_ok());
    set(&file, 0o666);
    assert_eq!(
        Hooks::load(&file).unwrap_err().id,
        "hooks.error.exposed_file"
    );
    let document = Document::read(&file);
    assert!(
        document.drafts().is_some(),
        "the editor still shows the rules"
    );
    set(&file, 0o644);
}

/// PRD 0013 S-02: no free text reaches the listing raw. A filter value or
/// a header path with a newline, an escape sequence or a bidi override is
/// shown escaped; a rule name with one is refused; a program whose real
/// path holds one cannot be pinned at all (stripping it would misstate
/// what runs).
#[test]
fn the_listing_shows_disguising_characters_escaped() {
    let rules = Hooks::from_toml(
        PathBuf::from("/x/\u{1b}[2K\rhooks.toml"),
        "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\naccount = \"Work\\u202e\"\ncommand = [\"true\", \"x\\u0085y\"]\n",
    )
    .unwrap()
    .with_search_path(None);
    let shown: Vec<String> = rules
        .listing()
        .iter()
        .map(|line| line.resolve("en-US"))
        .collect();
    for line in &shown {
        assert!(!line.chars().any(crate::text::is_disguising), "{line:?}");
    }
    let all = shown.join("\n");
    assert!(all.contains("\\u{1b}[2K\\u{d}hooks.toml"), "{all}");
    assert!(all.contains("Work\\u{202e}"), "{all}");
    assert!(all.contains("x\\u{85}y"), "{all}");
    assert_eq!(
        error_id("[[hooks]]\nname = \"a\\u202e\"\nevents = [\"quota_exhausted\"]\ncommand = [\"true\"]\n"),
        "hooks.error.name"
    );
}

#[cfg(unix)]
#[test]
fn a_program_whose_path_disguises_it_cannot_be_approved() {
    let root = tempfile::tempdir().unwrap();
    for name in ["evil\u{1b}[2K\rtool", "tool\u{202e}txt.sh"] {
        let real = root.path().join(name);
        write_program(&real, b"x");
        let link = root.path().join("link");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let rules = Hooks::from_toml(
            root.path().join("hooks.toml"),
            &format!(
                "[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = [{:?}]\n",
                link.display().to_string()
            ),
        )
        .unwrap()
        .with_search_path(None);
        assert_eq!(rules.pins()[0], Err(Unresolved::Disguised), "{name:?}");
        assert_eq!(rules.approval().unwrap_err().id, "hooks.error.disguised");
    }
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
            let hooks = Hooks::from_toml(self.path("hooks.toml"), &text)
                .unwrap()
                .with_search_path(Some("/usr/bin:/bin".into()));
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
                Some(HookClaims::at(
                    self.path("data").join("hooks").join("claims.json"),
                )),
            );
            (runner, receiver)
        }
    }

    /// `event` as a transition never seen before (each call is a new
    /// reading), so the claims never stand in the way of these tests.
    fn fresh(event: &Event) -> ObservedEvent {
        use std::sync::atomic::{AtomicU64, Ordering};
        static READING: AtomicU64 = AtomicU64::new(0);
        let n = READING.fetch_add(1, Ordering::Relaxed);
        ObservedEvent {
            event: event.clone(),
            transition: crate::watch::Transition::observed(format!("test {n}"), event.occurred_at),
        }
    }

    /// [`Runner::submit`] of a new transition; the runs dropped.
    fn submit(runner: &Runner, event: &Event) -> usize {
        submit_in(runner, event, false)
    }

    fn submit_in(runner: &Runner, event: &Event, in_quiet: bool) -> usize {
        let submitted = runner.submit_in(&fresh(event), in_quiet);
        assert_eq!(submitted.unclaimed, 0);
        submitted.dropped
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
        assert_eq!(submit(&runner, &event), 0);
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
        assert_eq!(vars["CUOTAI_ACCOUNT_ID"], "acc_test00000000");
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
        // Present when confirmed, gone when its event comes (PRD 0050
        // RF2.4: a program that no longer resolves to its pin never runs).
        let absent = rig.script("absent", "exit 0\n");
        let confirmed = rig.confirmed(&[
            ("sleep", "\"quota_exhausted\"", sh(&sleeper, &[]), 1),
            ("chatty", "\"quota_reset\"", sh(&chatty, &[]), 10),
            ("fail", "\"refresh_failed\"", sh(&failing, &[]), 10),
            ("missing", "\"auth_required\"", vec![absent.clone()], 10),
            (
                "after",
                "\"usage_updated\"",
                vec!["/bin/sh".into(), "-c".into(), "exit 0".into()],
                10,
            ),
        ]);
        let (runner, mut runs) = rig.start(confirmed, vec![("PATH", "/usr/bin:/bin")]);
        std::fs::remove_file(&absent).unwrap();

        submit(&runner, &event(EventKind::QuotaExhausted, "Work", None));
        let run = next(&mut runs).await;
        assert_eq!(
            (run.rule.as_str(), run.outcome),
            ("sleep", Outcome::TimedOut)
        );
        assert_eq!(run.exit_code, None);
        assert!(run.duration < Duration::from_secs(5), "{run:?}");
        let pid = std::fs::read_to_string(&pid).unwrap();
        assert!(!alive(&pid), "the timed-out process was killed");

        submit(&runner, &event(EventKind::QuotaReset, "Work", None));
        let run = next(&mut runs).await;
        assert!(run.succeeded(), "{run:?}");
        assert_eq!(run.output.len(), OUTPUT_LIMIT);
        assert!(run.truncated);

        submit(&runner, &event(EventKind::RefreshFailed, "Work", None));
        let run = next(&mut runs).await;
        assert_eq!((run.outcome, run.exit_code), (Outcome::Exited, Some(3)));
        assert_eq!(run.output, "oops\n");
        assert!(!run.truncated);

        submit(&runner, &event(EventKind::AuthRequired, "Work", None));
        assert_eq!(next(&mut runs).await.outcome, Outcome::Changed);

        submit(&runner, &event(EventKind::UsageUpdated, "Work", None));
        assert!(next(&mut runs).await.succeeded(), "the runner goes on");

        let log = std::fs::read_to_string(rig.path("runs.log")).unwrap();
        let outcomes: Vec<serde_json::Value> = log
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap()["outcome"].clone())
            .collect();
        assert_eq!(
            outcomes,
            ["timed_out", "exited", "exited", "changed", "exited"]
        );
    }

    /// PRD 0050 RF2.4: a hook runs the exact file pinned at consent. A bare
    /// name found on the captured `PATH` runs (with its written name as
    /// `argv[0]`); new bytes in a pinned program, or another file of the
    /// name appearing earlier on `PATH`, run nothing and suspend the rule,
    /// which stays suspended even when the original comes back.
    // security-invariant: SEC-HOOK-PIN
    #[tokio::test]
    async fn a_swapped_program_never_runs_and_the_rule_stays_suspended() {
        let rig = Rig::new();
        let (early, bin) = (rig.path("early"), rig.path("bin"));
        std::fs::create_dir(&early).unwrap();
        std::fs::create_dir(&bin).unwrap();
        // A link to the system shell: what runs is never a file this test
        // wrote.
        std::os::unix::fs::symlink("/bin/sh", bin.join("tool")).unwrap();
        // A copy that is changed before it would ever run.
        std::fs::copy("/bin/sh", bin.join("copy")).unwrap();
        let ran = rig.path("ran");
        let copied = rig.path("copied");
        let rule = |name: &str, event: &str, program: &str, marker: &Path| {
            format!(
                "[[hooks]]\nname = \"{name}\"\nevents = [\"{event}\"]\ncommand = {}\n",
                serde_json::to_string(&[
                    program,
                    "-c",
                    &format!("printf '%s' \"$0\" > '{}'", marker.display()),
                ])
                .unwrap()
            )
        };
        let text = rule("pinned", "quota_exhausted", "tool", &ran)
            + &rule("copied", "quota_reset", "copy", &copied);
        let search =
            std::env::join_paths([&early, &bin, Path::new("/usr/bin"), Path::new("/bin")]).unwrap();
        let hooks = Hooks::from_toml(rig.path("hooks.toml"), &text)
            .unwrap()
            .with_search_path(Some(search.clone()));
        let confirmed = Consent::at(rig.path("consent.json"))
            .confirm(hooks)
            .unwrap();
        assert_eq!(
            confirmed.pins()[0].program,
            std::fs::canonicalize("/bin/sh").unwrap()
        );
        // The parent's own PATH is not what the hooks get.
        let (runner, mut runs) = rig.start(confirmed, vec![("PATH", "/nowhere")]);

        submit(&runner, &event(EventKind::QuotaExhausted, "Work", None));
        let run = next(&mut runs).await;
        assert!(run.succeeded(), "{run:?}");
        assert_eq!(std::fs::read_to_string(&ran).unwrap(), "tool", "argv[0]");
        std::fs::remove_file(&ran).unwrap();

        // New bytes in the pinned copy.
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(bin.join("copy"))
            .unwrap();
        file.write_all(b"\n").unwrap();
        drop(file);
        submit(&runner, &event(EventKind::QuotaReset, "Work", None));
        let run = next(&mut runs).await;
        assert_eq!(
            (run.rule.as_str(), run.outcome),
            ("copied", Outcome::Changed)
        );
        assert!(!copied.exists(), "the changed program never ran");

        // Another file of the same name earlier on PATH.
        std::os::unix::fs::symlink("/usr/bin/touch", early.join("tool")).unwrap();
        submit(&runner, &event(EventKind::QuotaExhausted, "Work", None));
        let run = next(&mut runs).await;
        assert_eq!(
            (run.rule.as_str(), run.outcome),
            ("pinned", Outcome::Changed)
        );
        // Gone again: the rule stays suspended until a new confirmation.
        std::fs::remove_file(early.join("tool")).unwrap();
        submit(&runner, &event(EventKind::QuotaExhausted, "Work", None));
        assert_eq!(next(&mut runs).await.outcome, Outcome::Changed);
        assert!(!ran.exists(), "nothing ran after the swap");

        let log = std::fs::read_to_string(rig.path("runs.log")).unwrap();
        assert_eq!(log.matches("\"outcome\":\"changed\"").count(), 3, "{log}");
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
                submit(
                    &runner,
                    &event(EventKind::QuotaExhausted, &format!("n{index}"), None),
                )
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
        submit(&runner, &event(EventKind::QuotaExhausted, "Work", None));
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

        submit(&runner, &event(EventKind::QuotaExhausted, "Work", None));
        let run = next(&mut runs).await;
        assert_eq!(run.outcome, Outcome::TimedOut);
        let grandchild = read_pid(&hung).await;
        gone(&grandchild, "the timed-out hook's grandchild survived").await;

        submit(&runner, &event(EventKind::QuotaReset, "Work", None));
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
        submit(&runner, &event(EventKind::QuotaExhausted, "Work", None));
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

    /// PRD 0032 RF5.1: a `skip` rule does not run for an event inside a
    /// quiet window, a `run` rule always does, and both run outside one.
    #[tokio::test]
    async fn a_skip_rule_stays_silent_inside_quiet_hours() {
        let rig = Rig::new();
        let quiet_file = rig.path("quiet-ran");
        let always_file = rig.path("always-ran");
        let quiet_script = rig.script("q.sh", &format!("touch '{}'\n", quiet_file.display()));
        let always_script = rig.script("a.sh", &format!("touch '{}'\n", always_file.display()));
        let toml = format!(
            "[[hooks]]\nname = \"quiet\"\nevents = [\"refresh_failed\"]\nquiet = \"skip\"\ncommand = {}\n\n[[hooks]]\nname = \"always\"\nevents = [\"refresh_failed\"]\ncommand = {}\n",
            serde_json::to_string(&sh(&quiet_script, &[])).unwrap(),
            serde_json::to_string(&sh(&always_script, &[])).unwrap(),
        );
        let hooks = Hooks::from_toml(rig.path("hooks.toml"), &toml).unwrap();
        let confirmed = Consent::at(rig.path("consent.json"))
            .confirm(hooks)
            .unwrap();
        let (runner, mut runs) = rig.start(confirmed, vec![("PATH", "/usr/bin:/bin")]);
        let event = runner.rules()[0].sample(Utc::now());
        assert_eq!(submit_in(&runner, &event, true), 0);
        let run = next(&mut runs).await;
        assert_eq!(run.rule, "always");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(always_file.exists() && !quiet_file.exists());
        // Outside quiet hours both run.
        submit_in(&runner, &event, false);
        next(&mut runs).await;
        next(&mut runs).await;
        assert!(quiet_file.exists());
    }

    /// PRD 0054 RF2: two runners on one claim store (the desktop and
    /// `cuotai --watch --hooks`, which share only these files) run each
    /// rule once for the same transition, and again for a later distinct
    /// reading; the Test action bypasses the claims.
    #[tokio::test]
    async fn two_processes_run_each_rule_once_per_transition() {
        let rig = Rig::new();
        let log = rig.path("ran");
        let append = |name: &str| {
            rig.script(
                &format!("{name}.sh"),
                &format!("echo {name} >> '{}'\n", log.display()),
            )
        };
        let (a, b) = (append("a"), append("b"));
        let rules = [
            ("a", "\"quota_exhausted\"", sh(&a, &[]), 10),
            ("b", "\"quota_exhausted\"", sh(&b, &[]), 10),
        ];
        let (desktop, mut desktop_runs) =
            rig.start(rig.confirmed(&rules), vec![("PATH", "/usr/bin:/bin")]);
        let (watch, mut watch_runs) =
            rig.start(rig.confirmed(&rules), vec![("PATH", "/usr/bin:/bin")]);
        let exhausted = event(EventKind::QuotaExhausted, "Work", None);
        let reading = |at: i64| ObservedEvent {
            event: exhausted.clone(),
            transition: crate::watch::Transition::observed(
                "claude quota_exhausted",
                Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap()
                    + chrono::TimeDelta::seconds(at),
            ),
        };
        let first = desktop.submit(&reading(0));
        assert_eq!((first.queued, first.claimed_elsewhere), (2, 0));
        // The same reading, observed seconds later by the other process.
        let mut late = reading(0);
        late.event.occurred_at += chrono::TimeDelta::seconds(7);
        let second = watch.submit(&late);
        assert_eq!((second.queued, second.claimed_elsewhere), (0, 2));
        next(&mut desktop_runs).await;
        next(&mut desktop_runs).await;
        // A later distinct reading runs again, in whichever process sees it first.
        assert_eq!(watch.submit(&reading(60)).queued, 2);
        assert_eq!(desktop.submit(&reading(60)).claimed_elsewhere, 2);
        next(&mut watch_runs).await;
        next(&mut watch_runs).await;
        // The explicit test always runs.
        let sample = watch.rules()[0].sample(Utc::now());
        assert_eq!(desktop.submit_to("a", &sample), Some(0));
        assert_eq!(watch.submit_to("a", &sample), Some(0));
        next(&mut desktop_runs).await;
        next(&mut watch_runs).await;
        let mut ran: Vec<String> = std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        ran.sort();
        assert_eq!(ran, ["a", "a", "a", "a", "b", "b"]);
    }

    /// PRD 0054 RF2.3: without a claim store nothing runs, and the caller
    /// learns it.
    #[tokio::test]
    async fn without_a_claim_store_no_hook_runs() {
        let rig = Rig::new();
        let ran = rig.path("ran");
        let touch = rig.script("t.sh", &format!("touch '{}'\n", ran.display()));
        let confirmed = rig.confirmed(&[("t", "\"quota_exhausted\"", sh(&touch, &[]), 10)]);
        let runner = Runner::start_with_env(
            confirmed,
            None,
            Arc::new(|_: &Run, _: Option<&io::Error>| {}),
            vec![(OsString::from("PATH"), OsString::from("/usr/bin:/bin"))],
            Some(rig.path("work")),
            None,
        );
        let submitted = runner.submit(&fresh(&event(EventKind::QuotaExhausted, "Work", None)));
        assert_eq!((submitted.queued, submitted.unclaimed), (0, 1));
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!ran.exists());
    }

    /// One rule running `program` alone for `quota_exhausted`, confirmed.
    fn script_rule(rig: &Rig, program: &Path) -> Confirmed {
        let text = format!(
            "[[hooks]]\nname = \"s\"\nevents = [\"quota_exhausted\"]\ncommand = [{:?}, \"arg\"]\n",
            program.display().to_string()
        );
        let hooks = Hooks::from_toml(rig.path("hooks.toml"), &text)
            .unwrap()
            .with_search_path(Some("/usr/bin:/bin".into()));
        Consent::at(rig.path("consent.json"))
            .confirm(hooks)
            .unwrap()
    }

    fn write_script(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::rename(&tmp, path).unwrap();
    }

    /// PRD 0013 S-01: a script runs from a private verified copy through
    /// its pinned interpreter (its `$0` is the copy, its arguments are the
    /// rule's), and the copy is gone after the run.
    // security-invariant: SEC-HOOK-PIN
    #[tokio::test]
    async fn a_script_runs_from_a_private_copy() {
        let rig = Rig::new();
        let script = rig.path("hook");
        let marker = rig.path("marker");
        write_script(
            &script,
            &format!("printf '%s %s' \"$0\" \"$1\" > '{}'", marker.display()),
        );
        let (runner, mut runs) = rig.start(script_rule(&rig, &script), Vec::new());
        submit(&runner, &event(EventKind::QuotaExhausted, "Work", None));
        let run = next(&mut runs).await;
        assert!(run.succeeded(), "{run:?}");
        let seen = std::fs::read_to_string(&marker).unwrap();
        let exec = rig.path("data").join("hooks").join("exec");
        let (zero, arg) = seen.rsplit_once(' ').unwrap();
        assert!(Path::new(zero).starts_with(&exec), "{seen}");
        assert!(zero.ends_with("/hook"), "{seen}");
        assert_eq!(arg, "arg");
        assert_eq!(std::fs::read_dir(&exec).unwrap().count(), 0, "copy removed");
    }

    /// PRD 0013 S-01: a script made writable by others after approval is
    /// not run, and its rule stays suspended.
    #[tokio::test]
    async fn a_program_exposed_after_approval_never_runs() {
        use std::os::unix::fs::PermissionsExt;
        let rig = Rig::new();
        let script = rig.path("hook");
        let marker = rig.path("marker");
        write_script(&script, &format!(": > '{}'", marker.display()));
        let (runner, mut runs) = rig.start(script_rule(&rig, &script), Vec::new());
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o757)).unwrap();
        submit(&runner, &event(EventKind::QuotaExhausted, "Work", None));
        assert_eq!(next(&mut runs).await.outcome, Outcome::Exposed);
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        submit(&runner, &event(EventKind::QuotaExhausted, "Work", None));
        assert_eq!(
            next(&mut runs).await.outcome,
            Outcome::Changed,
            "still suspended"
        );
        assert!(!marker.exists());
    }

    /// PRD 0013 S-01, the Security harness: a script swapped back and forth
    /// with another while its rule runs never runs the other one (0 of N),
    /// since the interpreter reads a verified private copy.
    // security-invariant: SEC-HOOK-PIN
    #[tokio::test]
    async fn a_script_swapped_while_running_never_runs_the_swapped_bytes() {
        let rig = Rig::new();
        let script = rig.path("hook");
        let (good, evil) = (rig.path("good"), rig.path("evil"));
        write_script(&script, &format!("echo x >> '{}'", good.display()));
        let (runner, mut runs) = rig.start(script_rule(&rig, &script), Vec::new());
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let swapper = {
            let (stop, script) = (Arc::clone(&stop), script.clone());
            let (good, evil) = (good.clone(), evil.clone());
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    write_script(&script, &format!("echo x >> '{}'", evil.display()));
                    write_script(&script, &format!("echo x >> '{}'", good.display()));
                }
            })
        };
        let mut outcomes = Vec::new();
        for _ in 0..20 {
            submit(&runner, &event(EventKind::QuotaExhausted, "Work", None));
            outcomes.push(next(&mut runs).await.outcome);
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        swapper.join().unwrap();
        assert!(!evil.exists(), "the swapped script ran: {outcomes:?}");
        assert!(outcomes
            .iter()
            .all(|outcome| matches!(outcome, Outcome::Exited | Outcome::Changed)));
    }

    /// PRD 0013 S-04: a NUL (or another control character) in the metric
    /// label, which the provider controls, no longer stops the hook from
    /// starting: the variable gets U+FFFD, the payload the exact text.
    #[tokio::test]
    async fn a_control_character_in_a_label_still_starts_the_hook() {
        let rig = Rig::new();
        let out = rig.path("out");
        let script = rig.script(
            "env.sh",
            &format!("printf '%s' \"$CUOTAI_METRIC\" > '{}'", out.display()),
        );
        let confirmed = rig.confirmed(&[("env", "\"quota_exhausted\"", sh(&script, &[]), 10)]);
        let (runner, mut runs) = rig.start(confirmed, Vec::new());
        let mut sent = event(EventKind::QuotaExhausted, "Work", None);
        sent.metric = Some(MetricRef::new("a\0b\nc", None));
        submit(&runner, &sent);
        let run = next(&mut runs).await;
        assert!(run.succeeded(), "{run:?}");
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            "a\u{fffd}b\u{fffd}c"
        );
    }
}

/// PRD 0029 RF4.3: `account_id` filters by the stable id, which a rename
/// does not change, and the sample event of such a rule passes it.
#[test]
fn a_rule_can_filter_by_account_id() {
    let hooks = hooks(
        "[[hooks]]\nname = \"id\"\nevents = [\"quota_exhausted\"]\naccount_id = \"acc_test00000000\"\ncommand = [\"true\"]\n",
    );
    let rule = &hooks.rules()[0];
    assert_eq!(rule.account_id.as_deref(), Some("acc_test00000000"));
    assert!(rule.matches(&event(EventKind::QuotaExhausted, "Renamed", None)));
    let mut other = event(EventKind::QuotaExhausted, "Renamed", None);
    other.account_id = "acc_other0000000".into();
    assert!(!rule.matches(&other));
    let now = Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap();
    assert!(rule.matches(&rule.sample(now)));
}

/// PRD 0036 RF7.3: a rule on an aggregator account also runs on the events
/// of its connections (`parent`), and on nothing else.
#[test]
fn a_rule_on_an_aggregator_covers_its_connections() {
    let rule = Rule {
        quiet: Default::default(),
        account_id: None,
        name: "pool".into(),
        events: vec![EventKind::QuotaExhausted, EventKind::PoolExhausted],
        provider: None,
        account: Some("home".into()),
        min_threshold: None,
        command: vec!["true".into()],
        timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
    };
    let mut connection = event(EventKind::QuotaExhausted, "home · a1", None);
    connection.parent = Some("nine_router:home".into());
    assert!(rule.matches(&connection));
    connection.parent = Some("nine_router:office".into());
    assert!(!rule.matches(&connection));
    connection.parent = None;
    assert!(!rule.matches(&connection));
    assert!(rule.matches(&event(EventKind::PoolExhausted, "home", None)));
}

/// PRD 0051 RF3: a later revision of this schema is read without its new
/// keys, and an editor save keeps them; a later major is refused and never
/// rewritten; this revision stays strict.
#[test]
fn hooks_toml_reads_a_later_revision_and_refuses_a_later_major() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hooks.toml");
    let later = "schema = 1\nschema_revision = 2\nfuture_top = true\n\n[[hooks]]\nname = \"sound\"\nevents = [\"quota_exhausted\"]\ncommand = [\"true\"]\nretries = 3 # a key of a later revision\n";
    std::fs::write(&path, later).unwrap();
    let document = Document::read(&path);
    assert_eq!(document.hooks().as_ref().unwrap().rules().len(), 1);
    let mut drafts = document.drafts().unwrap().to_vec();
    drafts[0].timeout_seconds = Some(5);
    Document::save(&path, document.revision(), drafts).unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(
        saved.contains("retries = 3 # a key of a later revision"),
        "{saved}"
    );
    assert!(saved.contains("schema_revision = 2") && saved.contains("future_top = true"));
    assert!(saved.contains("timeout_seconds = 5"));

    let newer = "schema = 2\n[[hooks]]\nname = \"x\"\n";
    std::fs::write(&path, newer).unwrap();
    let document = Document::read(&path);
    assert_eq!(
        document.hooks().as_ref().unwrap_err().id,
        "state.error.newer_major"
    );
    assert!(Document::save(&path, document.revision(), vec![draft("x", &["true"])]).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), newer);

    assert_eq!(
        error_id("schema = 1\n[[hooks]]\nname = \"a\"\nevents = [\"quota_exhausted\"]\ncommand = [\"true\"]\nretries = 3\n"),
        "hooks.error.toml",
        "the current revision is strict"
    );
}
