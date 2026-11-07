use super::*;
use std::collections::HashMap;

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new(config: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        if let Some(text) = config {
            std::fs::write(dir.path().join("cuotai.toml"), text).unwrap();
        }
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        Self { dir }
    }

    fn inputs(&self, vars: &[(&str, &str)], port: u16) -> Inputs {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Inputs {
            config: ResolvedPath {
                path: self.dir.path().join("cuotai.toml"),
                source: PathSource::Argument,
            },
            var: Box::new(move |name| vars.get(name).cloned()),
            data_override: Ok(None),
            data_root: Some(self.dir.path().join("data")),
            desktop_settings: Some(self.dir.path().join("desktop.json")),
            port,
            version: "9.9.9".into(),
            install_source: "test".into(),
            resolver: crate::external_tool::Resolver {
                path: self.dir.path().join("path-dir").into_os_string(),
                home: Some(self.dir.path().join("home")),
                program_files: None,
                os: crate::external_tool::Os::Linux,
                sandbox: Some(self.dir.path().to_path_buf()),
            },
            vault: Box::new(|| true),
            extensions_dir: Some(self.dir.path().join("extensions")),
            cost_env: crate::cost::Env::new(Some(self.dir.path().join("home")), []),
            clock: Box::new(ClockState::default),
            notifications: None,
            locale: "pt-BR".into(),
            now: Utc::now(),
            log_health: LogHealth::Ok,
            proxy: crate::network::proxy_state_from(|_| None),
            trust_roots: Some(42),
        }
    }
}

fn free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn status(report: &Report, id: &str) -> CheckStatus {
    report
        .checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("no check {id}"))
        .status
}

const TWO_ACCOUNTS: &str = "[[accounts]]\nname = \"Personal Groq\"\nprovider = \"groq\"\ncredential_env = \"MY_GROQ\"\n\n[[accounts]]\nname = \"Work\"\nprovider = \"mistral\"\n";

#[test]
fn a_missing_variable_fails_and_a_set_one_never_shows_its_value() {
    let fixture = Fixture::new(Some(TWO_ACCOUNTS));
    let report = run(&fixture.inputs(&[], free_port()));
    assert_eq!(status(&report, "config"), CheckStatus::Ok);
    assert_eq!(status(&report, "account.1"), CheckStatus::Fail);
    assert_eq!(
        status(&report, "account.2"),
        CheckStatus::Warn,
        "default variable unset"
    );
    assert_eq!(report.exit_code(), 2);

    let report = run(&fixture.inputs(
        &[
            ("MY_GROQ", "sk-live-SEEDED-VALUE"),
            ("MISTRAL_API_KEY", "SEEDED-2"),
        ],
        free_port(),
    ));
    assert_eq!(status(&report, "account.1"), CheckStatus::Ok);
    assert_eq!(status(&report, "account.2"), CheckStatus::Ok);
    assert_eq!(report.exit_code(), 0);
    let json = serde_json::to_string(&report).unwrap();
    let text: String = report
        .checks
        .iter()
        .flat_map(|c| [Some(&c.title), c.detail.as_ref(), c.fix.as_ref()])
        .flatten()
        .map(|note| note.resolve("en-US") + "\n")
        .collect();
    for leak in ["SEEDED", "Personal Groq", "Work"] {
        assert!(!json.contains(leak), "{leak} in {json}");
        assert!(!text.contains(leak), "{leak} in {text}");
    }
    assert!(
        text.contains("MY_GROQ"),
        "the variable name is shown: {text}"
    );
}

#[test]
fn a_broken_or_missing_configuration_is_reported_with_its_fix() {
    let missing = Fixture::new(None);
    let report = run(&missing.inputs(&[], free_port()));
    assert_eq!(status(&report, "config"), CheckStatus::Warn);
    assert_eq!(report.exit_code(), 0);

    let broken = Fixture::new(Some(
        "[[accounts]]\nprovider = \"groq\"\nrefresh_seconds = \"x\"\n",
    ));
    let report = run(&broken.inputs(&[], free_port()));
    let check = report.checks.iter().find(|c| c.id == "config").unwrap();
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(check.detail.as_ref().unwrap().id, "config.error.parse_key");
    assert_eq!(check.fix.as_ref().unwrap().id, "doctor.fix.check_config");
    assert_eq!(report.exit_code(), 2);

    let newer = Fixture::new(Some(
        "[[accounts]]\nprovider = \"from_the_future\"\n\n[[accounts]]\nprovider = \"groq\"\ncredential_env = \"G\"\n",
    ));
    let report = run(&newer.inputs(&[("G", "x")], free_port()));
    assert_eq!(status(&report, "config"), CheckStatus::Warn);
    assert_eq!(status(&report, "account.1"), CheckStatus::Ok);
}

#[test]
fn a_taken_port_warns_and_a_broken_hooks_file_fails() {
    let fixture = Fixture::new(Some(TWO_ACCOUNTS));
    let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = held.local_addr().unwrap().port();
    std::fs::write(
        fixture.dir.path().join("hooks.toml"),
        "[[hooks]]\nname = 3\n",
    )
    .unwrap();
    let report = run(&fixture.inputs(&[], port));
    assert_eq!(status(&report, "port"), CheckStatus::Warn);
    assert_eq!(status(&report, "hooks"), CheckStatus::Fail);
}

#[test]
fn stores_are_checked_read_only_and_a_newer_major_is_reported() {
    let fixture = Fixture::new(Some(TWO_ACCOUNTS));
    let report = run(&fixture.inputs(&[], free_port()));
    for id in [
        "history",
        "models",
        "cost_index",
        "serve_token",
        "desktop_settings",
    ] {
        assert_eq!(status(&report, id), CheckStatus::Skip, "{id}");
    }
    let history = fixture.dir.path().join("data/history/history.sqlite3");
    crate::history::HistoryStore::new(
        history.clone(),
        &crate::config_file::ConfigIdentity::Explicit("x".into()),
    )
    .bounds()
    .unwrap();
    let before = std::fs::read(&history).unwrap();
    let report = run(&fixture.inputs(&[], free_port()));
    assert_eq!(status(&report, "history"), CheckStatus::Ok);
    assert_eq!(std::fs::read(&history).unwrap(), before, "read-only");
    rusqlite::Connection::open(&history)
        .unwrap()
        .execute_batch("UPDATE state_meta SET value = 2 WHERE key = 'major'")
        .unwrap();
    let report = run(&fixture.inputs(&[], free_port()));
    assert_eq!(status(&report, "history"), CheckStatus::Warn);

    let models = fixture.dir.path().join("data/models/models.sqlite3");
    std::fs::create_dir_all(models.parent().unwrap()).unwrap();
    std::fs::write(
        &models,
        b"this is not a database at all, not even close......",
    )
    .unwrap();
    let report = run(&fixture.inputs(&[], free_port()));
    assert_eq!(status(&report, "models"), CheckStatus::Fail);

    std::fs::write(fixture.dir.path().join("desktop.json"), "{broken").unwrap();
    let report = run(&fixture.inputs(&[], free_port()));
    assert_eq!(status(&report, "desktop_settings"), CheckStatus::Warn);
}

#[cfg(unix)]
#[test]
fn a_readable_server_token_fails() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new(Some(TWO_ACCOUNTS));
    let token = fixture.dir.path().join("data/serve/token");
    std::fs::create_dir_all(token.parent().unwrap()).unwrap();
    std::fs::write(&token, "SEEDED-TOKEN").unwrap();
    std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o644)).unwrap();
    let report = run(&fixture.inputs(&[], free_port()));
    assert_eq!(status(&report, "serve_token"), CheckStatus::Fail);
    std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
    let report = run(&fixture.inputs(&[], free_port()));
    assert_eq!(status(&report, "serve_token"), CheckStatus::Ok);
    assert!(!serde_json::to_string(&report).unwrap().contains("SEEDED"));
}

#[test]
fn an_invalid_data_folder_fails() {
    let fixture = Fixture::new(Some(TWO_ACCOUNTS));
    let mut inputs = fixture.inputs(&[], free_port());
    inputs.data_override = Err(crate::data_dir::DataDirError::Relative);
    inputs.data_root = None;
    let report = run(&inputs);
    assert_eq!(status(&report, "data_dir"), CheckStatus::Fail);
    assert_eq!(report.exit_code(), 2);
}

/// PRD 0054 RF6.3: a CLI-backed account reports where its program is, or
/// that it is missing, with the home shown as `~` and no PATH value.
#[cfg(unix)]
#[test]
fn a_cli_backed_account_reports_its_program() {
    use std::os::unix::fs::PermissionsExt;
    let text = "[[accounts]]\nname = \"Bedrock\"\nprovider = \"aws_bedrock\"\n[accounts.params]\nprofile = \"prod\"\n";
    let fixture = Fixture::new(Some(text));
    let report = run(&fixture.inputs(&[], free_port()));
    assert_eq!(status(&report, "account.1.tool"), CheckStatus::Fail);
    let tools = serde_json::to_value(&report.tools).unwrap();
    assert_eq!(tools["aws"]["status"], "missing");
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("path-dir"), "PATH is never printed: {json}");

    let bin = fixture.dir.path().join("path-dir");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("aws"), b"").unwrap();
    std::fs::set_permissions(bin.join("aws"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let report = run(&fixture.inputs(&[], free_port()));
    assert_eq!(status(&report, "account.1.tool"), CheckStatus::Ok);
    let tools = serde_json::to_value(&report.tools).unwrap();
    assert_eq!(tools["aws"]["status"], "found");
    assert_eq!(tools["aws"]["source"], "path");
}

/// PRD 0003 COMP-02: a set-aside ledger is reported until it is deleted.
#[test]
fn a_set_aside_ledger_is_reported() {
    let fixture = Fixture::new(Some(TWO_ACCOUNTS));
    let data = fixture.dir.path().join("data");
    let report = run(&fixture.inputs(&[], free_port()));
    assert_eq!(status(&report, "ledger"), CheckStatus::Skip);
    std::fs::write(data.join("notified.json"), "{}").unwrap();
    let report = run(&fixture.inputs(&[], free_port()));
    assert_eq!(status(&report, "ledger"), CheckStatus::Ok);
    std::fs::write(data.join("notified.json.corrupt-20261107T120000Z"), "x").unwrap();
    let report = run(&fixture.inputs(&[], free_port()));
    assert_eq!(status(&report, "ledger"), CheckStatus::Warn);
}

const VAULT_ACCOUNT: &str = "[[accounts]]\nname = \"Mine\"\nprovider = \"chat_gpt\"\n[accounts.oauth]\ncredential_id = \"4be708d8-7073-4d01-85ba-3306cc3cf087\"\n";

#[test]
fn an_unavailable_vault_fails_only_when_an_account_uses_it() {
    let fixture = Fixture::new(Some(VAULT_ACCOUNT));
    let mut inputs = fixture.inputs(&[], free_port());
    inputs.vault = Box::new(|| false);
    let report = run(&inputs);
    assert_eq!(status(&report, "vault"), CheckStatus::Fail);
    assert_eq!(report.exit_code(), 2);
    let mut inputs = fixture.inputs(&[], free_port());
    inputs.vault = Box::new(|| true);
    assert_eq!(status(&run(&inputs), "vault"), CheckStatus::Ok);

    let fixture = Fixture::new(Some(TWO_ACCOUNTS));
    let mut inputs = fixture.inputs(&[], free_port());
    inputs.vault = Box::new(|| panic!("the vault is not asked without a vault account"));
    assert_eq!(status(&run(&inputs), "vault"), CheckStatus::Skip);
}

#[test]
fn the_clock_warns_on_a_server_skew_or_a_reading_from_the_future() {
    let fixture = Fixture::new(None);
    let mut inputs = fixture.inputs(&[], free_port());
    inputs.clock = Box::new(|| ClockState {
        warning: Some(crate::network::Skew {
            direction: crate::network::SkewDirection::Behind,
            minutes: 12,
        }),
        observed: true,
    });
    assert_eq!(status(&run(&inputs), "clock"), CheckStatus::Warn);
    let mut inputs = fixture.inputs(&[], free_port());
    inputs.clock = Box::new(|| ClockState {
        warning: None,
        observed: true,
    });
    assert_eq!(status(&run(&inputs), "clock"), CheckStatus::Ok);
    // Nothing compared and no history: not checked.
    assert_eq!(
        status(&run(&fixture.inputs(&[], free_port())), "clock"),
        CheckStatus::Skip
    );
    // A stored reading an hour ahead of this clock: it went back.
    let dir = fixture.dir.path().join("data").join("history");
    std::fs::create_dir_all(&dir).unwrap();
    let db = rusqlite::Connection::open(dir.join("history.sqlite3")).unwrap();
    db.execute_batch("CREATE TABLE samples (series_id INTEGER, taken_at INTEGER);")
        .unwrap();
    let ahead = (Utc::now() + chrono::TimeDelta::hours(1)).timestamp_millis();
    db.execute("INSERT INTO samples VALUES (1, ?1)", [ahead])
        .unwrap();
    drop(db);
    assert_eq!(
        status(&run(&fixture.inputs(&[], free_port())), "clock"),
        CheckStatus::Warn
    );
}

#[test]
fn notifications_locale_and_cost_sources_are_reported() {
    let fixture = Fixture::new(None);
    let report = run(&fixture.inputs(&[], free_port()));
    assert!(
        report.checks.iter().all(|c| c.id != "notifications"),
        "only the desktop knows the permission"
    );
    let locale = report.checks.iter().find(|c| c.id == "locale").unwrap();
    assert!(locale
        .title
        .resolve("en-US")
        .starts_with("Language pt-BR, time zone UTC"));

    let mut inputs = fixture.inputs(&[], free_port());
    inputs.notifications = Some(false);
    assert_eq!(status(&run(&inputs), "notifications"), CheckStatus::Warn);
    let mut inputs = fixture.inputs(&[], free_port());
    inputs.notifications = Some(true);
    assert_eq!(status(&run(&inputs), "notifications"), CheckStatus::Ok);

    // A damaged cost settings file warns; a source that is on is listed.
    let cost = fixture.dir.path().join("data").join("cost");
    std::fs::create_dir_all(&cost).unwrap();
    std::fs::write(cost.join("settings.json"), "{not json").unwrap();
    assert_eq!(
        status(&run(&fixture.inputs(&[], free_port())), "cost_settings"),
        CheckStatus::Warn
    );
    std::fs::write(cost.join("settings.json"), "{}").unwrap();
    let report = run(&fixture.inputs(&[], free_port()));
    assert!(report
        .checks
        .iter()
        .all(|c| !c.id.starts_with("cost_source") && c.id != "cost_settings"));
}

#[test]
fn an_extension_account_needs_its_approval() {
    let fixture = Fixture::new(Some(
        "[[accounts]]\nname = \"Ext\"\nprovider = \"extension\"\n[accounts.params]\nextension_id = \"my-ext\"\n",
    ));
    let report = run(&fixture.inputs(&[], free_port()));
    assert_ne!(status(&report, "account.1"), CheckStatus::Ok);
    assert_ne!(status(&report, "account.1"), CheckStatus::Skip);
}

#[test]
fn each_channel_reports_its_last_outcome_without_its_label_or_host() {
    let fixture = Fixture::new(None);
    std::fs::write(
        fixture.dir.path().join("desktop.json"),
        r#"{"channels":[{"id":"aaaa000000000001","kind":"ntfy","label":"SEEDED label","host":"seeded.example.com","created_at":"2026-11-01T00:00:00Z"},{"id":"bbbb000000000002","kind":"slack","label":"B","host":"hooks.slack.com","created_at":"2026-11-01T00:00:00Z"}]}"#,
    )
    .unwrap();
    let log = fixture.dir.path().join("data").join("channels");
    std::fs::create_dir_all(&log).unwrap();
    std::fs::write(
        log.join("log.jsonl"),
        "{\"at\":\"2026-11-07T10:00:00Z\",\"channel\":\"aaaa000000000001\",\"kind\":\"threshold\",\"outcome\":\"failed\",\"status\":500}\n\
         {\"at\":\"2026-11-07T10:00:00Z\",\"channel\":\"bbbb000000000002\",\"kind\":\"threshold\",\"outcome\":\"delivered\",\"status\":200}\n",
    )
    .unwrap();
    let report = run(&fixture.inputs(&[], free_port()));
    assert_eq!(status(&report, "channel.1"), CheckStatus::Warn);
    assert_eq!(status(&report, "channel.2"), CheckStatus::Ok);
    let json = serde_json::to_string(&report).unwrap();
    assert!(
        !json.contains("SEEDED") && !json.contains("seeded.example.com"),
        "{json}"
    );
}

/// PRD 0052 RF3 (option A): the log, proxy, trust-store and file
/// checks join the doctor without leaking proxy userinfo, the `NO_PROXY`
/// list, account names, e-mails or variable values.
#[test]
fn diagnostics_show_log_proxy_trust_and_files_without_secrets() {
    let fixture = Fixture::new(Some(
        "[[accounts]]\nname = \"me@example.com\"\nprovider = \"groq\"\ncredential_env = \"MY_GROQ\"\n\n[[accounts]]\nname = \"Personal Groq\"\nprovider = \"mistral\"\n",
    ));
    let mut inputs = fixture.inputs(&[("MY_GROQ", "sk-live-SEEDED-VALUE")], free_port());
    inputs.proxy = crate::network::proxy_state_from(|name| match name {
        "HTTPS_PROXY" => Some("http://alice:SEEDED-PASS@proxy.example:3128/path?q=SEEDED-Q".into()),
        "NO_PROXY" => Some("secret-host.internal,10.9.8.0/24".into()),
        _ => None,
    });
    let report = run(&inputs);
    for id in ["log", "proxy", "trust_store", "artifacts"] {
        assert_eq!(status(&report, id), CheckStatus::Ok, "{id}");
    }
    assert_eq!(
        report.artifacts.len(),
        crate::privacy::artifacts().len(),
        "every registry entry is listed"
    );
    let json = serde_json::to_string(&report).unwrap();
    assert!(
        json.contains("\"artifacts\":[{\"id\":\"config.accounts\""),
        "{json}"
    );
    let text: String = report
        .checks
        .iter()
        .flat_map(|c| [Some(&c.title), c.detail.as_ref(), c.fix.as_ref()])
        .flatten()
        .map(|note| note.resolve("en-US") + "\n")
        .collect();
    assert!(
        text.contains("http://proxy.example:3128 (from HTTPS_PROXY)"),
        "{text}"
    );
    assert!(text.contains("A NO_PROXY list is set."), "{text}");
    assert!(text.contains("42 certificates"), "{text}");
    for leak in [
        "SEEDED",
        "alice",
        "secret-host",
        "10.9.8.0",
        "me@example.com",
        "Personal Groq",
        "/path",
    ] {
        assert!(!json.contains(leak), "{leak} in {json}");
        assert!(!text.contains(leak), "{leak} in {text}");
    }
}

/// No root certificates and an unwritable log are reported; a missing
/// trust store is the one failure (the server-clock warning is the
/// `clock` check, tested above).
#[test]
fn an_empty_trust_store_and_a_broken_log_are_reported() {
    let fixture = Fixture::new(Some(TWO_ACCOUNTS));
    let mut inputs = fixture.inputs(&[("MY_GROQ", "x"), ("MISTRAL_API_KEY", "y")], free_port());
    assert_eq!(run(&inputs).exit_code(), 0);
    inputs.log_health = LogHealth::Error;
    let report = run(&inputs);
    assert_eq!(status(&report, "log"), CheckStatus::Warn);
    assert_eq!(report.exit_code(), 0, "warnings only");
    inputs.log_health = LogHealth::Off;
    inputs.trust_roots = None;
    let report = run(&inputs);
    assert_eq!(status(&report, "log"), CheckStatus::Skip);
    assert_eq!(status(&report, "trust_store"), CheckStatus::Fail);
    assert_eq!(report.exit_code(), 2);
    inputs.proxy = crate::network::proxy_state_from(|name| {
        (name == "https_proxy").then(|| "http://".to_string())
    });
    assert_eq!(status(&run(&inputs), "proxy"), CheckStatus::Warn);
}

#[cfg(unix)]
#[test]
fn a_cuotai_file_open_to_others_warns_by_id() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new(Some(TWO_ACCOUNTS));
    let ledger = fixture.dir.path().join("data/notified.json");
    std::fs::write(&ledger, "{}").unwrap();
    std::fs::set_permissions(&ledger, std::fs::Permissions::from_mode(0o644)).unwrap();
    let report = run(&fixture.inputs(&[], free_port()));
    let check = report.checks.iter().find(|c| c.id == "artifacts").unwrap();
    assert_eq!(check.status, CheckStatus::Warn);
    assert!(check
        .detail
        .as_ref()
        .unwrap()
        .resolve("en-US")
        .contains("notifications.ledger"));
    std::fs::set_permissions(&ledger, std::fs::Permissions::from_mode(0o400)).unwrap();
    let report = run(&fixture.inputs(&[], free_port()));
    assert_eq!(
        status(&report, "artifacts"),
        CheckStatus::Ok,
        "narrower is fine"
    );
}
