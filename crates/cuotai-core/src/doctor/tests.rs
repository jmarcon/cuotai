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
