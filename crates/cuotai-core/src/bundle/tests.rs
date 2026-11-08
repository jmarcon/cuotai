use super::*;

const TOML: &str = "# my accounts\n[[accounts]]\nid = \"acct-one\"\nname = \"Groq\"\nprovider = \"groq\"\ncredential_env = \"GROQ_WORK\"\n\n[[accounts]]\nid = \"acct-two\"\nname = \"Claude\"\nprovider = \"claude\"\n\n[accounts.oauth]\ncredential_id = \"claude-work\"\n";

struct Tree {
    dir: tempfile::TempDir,
}

impl Tree {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        Self { dir }
    }

    fn at(&self, path: &str) -> PathBuf {
        self.dir.path().join(path)
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.at(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn locations(&self) -> Locations {
        Locations {
            config: self.at("config/cuotai.toml"),
            desktop_settings: Some(self.at("config/desktop.json")),
            user_dir: Some(self.at("user")),
            data_root: Some(self.at("data")),
        }
    }

    fn full() -> Self {
        let tree = Self::new();
        tree.write("config/cuotai.toml", TOML);
        tree.write("config/hooks.toml", "[[hooks]]\nname = \"n\"\n");
        tree.write(
            "config/desktop.json",
            r#"{"motion":"off","settings_window":{"width":900},"config_file":"/Users/someone/c.toml","serve":{"enabled":true,"port":6740,"token":true}}"#,
        );
        tree.write(
            "data/cost/settings.json",
            r#"{"sources":{"claude_code":{"enabled":true,"paths":["/Users/someone/.claude"]}},"retention_days":60}"#,
        );
        tree.write(
            "data/cost/pricing.json",
            r#"{"currency":"USD","models":{}}"#,
        );
        tree.write("data/serve/token", "SEEDED-SERVER-TOKEN-0123456789abcdef");
        tree.write("data/history/history.sqlite3", "not copied");
        tree.write("user/themes/night.css", ":root{}");
        tree.write("user/locales/xx.json", "{}");
        tree.write("user/themes/notes.txt", "ignored");
        tree
    }
}

fn at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn the_manifest_lists_the_allow_list_only_and_is_deterministic() {
    let tree = Tree::full();
    let bundle = collect(&tree.locations(), at(), "9.9.9").unwrap();
    let paths: Vec<_> = bundle
        .manifest
        .files
        .iter()
        .map(|e| e.path.as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "config/cuotai.toml",
            "config/hooks.toml",
            "prefs/cost-settings.json",
            "prefs/desktop.json",
            "prefs/pricing.json",
            "user/locales/xx.json",
            "user/themes/night.css",
        ]
    );
    assert_eq!(bundle.manifest.excluded, EXCLUDED);
    assert_eq!(bundle.manifest.created_at, "2026-01-01T00:00:00Z");
    let zip = bundle.to_zip();
    assert_eq!(
        collect(&tree.locations(), at(), "9.9.9").unwrap().to_zip(),
        zip,
        "the same tree gives the same bytes"
    );
    let text = String::from_utf8_lossy(&zip);
    assert!(!text.contains("SEEDED"), "the server token never travels");
    let read = Bundle::from_zip(&zip).unwrap();
    assert_eq!(read.manifest, bundle.manifest);
    // The scrub rules.
    let desktop: serde_json::Value =
        serde_json::from_slice(&read.files["prefs/desktop.json"]).unwrap();
    assert!(desktop.get("settings_window").is_none());
    assert_eq!(desktop["config_file"], "@config");
    assert_eq!(desktop["serve"]["port"], 6740);
    let cost: serde_json::Value =
        serde_json::from_slice(&read.files["prefs/cost-settings.json"]).unwrap();
    assert!(cost.get("sources").is_none());
    assert_eq!(cost["retention_days"], 60);
    assert_eq!(
        read.files["config/cuotai.toml"],
        TOML.as_bytes(),
        "the TOML travels verbatim"
    );
}

#[test]
fn a_token_like_string_stops_the_backup() {
    for leak in [
        "sk-ant-admin01-abcdefghijklmnopqrstuvwxyz",
        "ghp_0123456789abcdefghijklmnop",
        "-----BEGIN PRIVATE KEY-----",
        "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTYifQ.c2lnbmF0dXJlLXZhbHVl",
    ] {
        let tree = Tree::full();
        tree.write("config/hooks.toml", &format!("# {leak}\n"));
        assert_eq!(
            collect(&tree.locations(), at(), "9.9.9").unwrap_err(),
            BundleError::Secret {
                path: "config/hooks.toml".into()
            },
            "{leak}"
        );
    }
    for fine in [
        "OPENAI_ADMIN_KEY",
        "credential_env = \"GROQ_API_KEY\"",
        "reference = \"cuotai.oauth/claude/a2\"",
        "task-runner",
        "sk-short",
    ] {
        assert!(!looks_secret(fine), "{fine}");
    }
}

#[test]
fn unsafe_unlisted_or_edited_archives_are_refused() {
    let tree = Tree::full();
    let bundle = collect(&tree.locations(), at(), "9.9.9").unwrap();
    let rebuild = |bundle: &Bundle| Bundle::from_zip(&bundle.to_zip());

    let mut traversal = bundle.clone();
    traversal
        .files
        .insert("user/themes/../../evil.css".into(), b"x".to_vec());
    traversal.manifest.files.push(Entry {
        path: "user/themes/../../evil.css".into(),
        sha256: sha256(b"x"),
        kind: "theme".into(),
    });
    assert_eq!(rebuild(&traversal).unwrap_err(), BundleError::Invalid);

    let mut absolute = bundle.clone();
    absolute.files.insert("/etc/passwd".into(), b"x".to_vec());
    absolute.manifest.files.push(Entry {
        path: "/etc/passwd".into(),
        sha256: sha256(b"x"),
        kind: "config".into(),
    });
    assert_eq!(rebuild(&absolute).unwrap_err(), BundleError::Invalid);

    let mut unlisted = bundle.clone();
    unlisted
        .files
        .insert("prefs/extra.json".into(), b"{}".to_vec());
    assert_eq!(rebuild(&unlisted).unwrap_err(), BundleError::Invalid);

    let mut edited = bundle.clone();
    edited
        .files
        .insert("config/cuotai.toml".into(), b"accounts = []\n".to_vec());
    assert_eq!(
        rebuild(&edited).unwrap_err(),
        BundleError::Digest {
            path: "config/cuotai.toml".into()
        }
    );
    assert_eq!(rebuild(&edited).unwrap_err().exit_code(), 2);

    let mut newer = bundle.clone();
    newer.manifest.schema = 2;
    assert_eq!(
        rebuild(&newer).unwrap_err(),
        BundleError::Schema { schema: 2 }
    );
}

#[test]
fn a_restore_on_a_fresh_machine_adds_everything_and_lists_what_is_missing() {
    let source = Tree::full();
    let bundle = Bundle::from_zip(
        &collect(&source.locations(), at(), "9.9.9")
            .unwrap()
            .to_zip(),
    )
    .unwrap();
    let fresh = Tree::new();
    let no_vars = |_: &str| None;
    let plan = plan(&bundle, &fresh.locations(), None, &no_vars).unwrap();
    assert!(plan.items.iter().all(|item| item.action == Action::Add));
    assert!(!plan.has_conflicts());
    let needs: Vec<_> = plan.needs.iter().map(|n| n.resolve("en-US")).collect();
    assert_eq!(
        needs,
        [
            "Set GROQ_WORK for account 1 (groq).",
            "Sign in again to account 2 (claude)."
        ]
    );
    let written = plan
        .apply(|_| panic!("no question for a fresh machine"))
        .unwrap();
    assert_eq!(written, 7);
    assert_eq!(
        std::fs::read_to_string(fresh.at("config/cuotai.toml")).unwrap(),
        TOML
    );
    let desktop: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fresh.at("config/desktop.json")).unwrap()).unwrap();
    assert!(
        desktop.get("config_file").is_none(),
        "a marker is never written"
    );
    assert!(fresh.at("user/themes/night.css").is_file());
    assert!(!fresh.at("data/serve/token").exists());

    // Restoring again changes nothing.
    let again = super::plan(&bundle, &fresh.locations(), None, &no_vars).unwrap();
    assert!(again.items.iter().all(|item| item.action == Action::Keep));
    assert_eq!(again.apply(|_| true).unwrap(), 0);

    let with_var = |name: &str| (name == "GROQ_WORK").then(|| "set".to_owned());
    let plan = super::plan(&bundle, &fresh.locations(), Some(Part::User), &with_var).unwrap();
    assert!(plan.items.iter().all(|item| item.path.starts_with("user/")));
    assert!(plan.needs.is_empty());
}

#[test]
fn a_conflicting_local_edit_is_planned_and_only_written_when_confirmed() {
    let source = Tree::full();
    let bundle = collect(&source.locations(), at(), "9.9.9").unwrap();
    let target = Tree::new();
    target.write(
        "config/cuotai.toml",
        "[[accounts]]\nid = \"acct-one\"\nname = \"Groq\"\nprovider = \"groq\"\ncredential_env = \"OTHER\"\n\n[[accounts]]\nid = \"acct-three\"\nprovider = \"mistral\"\n",
    );
    target.write(
        "config/desktop.json",
        r#"{"motion":"system","locale":"de","settings_window":{"width":1}}"#,
    );
    let none = |_: &str| None;
    let plan = plan(&bundle, &target.locations(), None, &none).unwrap();
    let config = plan
        .items
        .iter()
        .find(|i| i.path == "config/cuotai.toml")
        .unwrap();
    // The local file was written after the backup's date: a newer local edit.
    assert_eq!(config.action, Action::Skip);
    assert_eq!(
        config.accounts,
        Some(AccountChanges {
            added: 1,
            changed: 1,
            removed: 1
        })
    );
    assert!(plan.has_conflicts());
    let before = std::fs::read(target.at("config/cuotai.toml")).unwrap();
    plan.apply(|_| false).unwrap();
    assert_eq!(
        std::fs::read(target.at("config/cuotai.toml")).unwrap(),
        before
    );
    let desktop: serde_json::Value =
        serde_json::from_slice(&std::fs::read(target.at("config/desktop.json")).unwrap()).unwrap();
    assert_eq!(desktop["motion"], "system", "declined");

    let plan = super::plan(&bundle, &target.locations(), None, &none).unwrap();
    plan.apply(|_| true).unwrap();
    assert_eq!(
        std::fs::read_to_string(target.at("config/cuotai.toml")).unwrap(),
        TOML
    );
    let desktop: serde_json::Value =
        serde_json::from_slice(&std::fs::read(target.at("config/desktop.json")).unwrap()).unwrap();
    assert_eq!(desktop["motion"], "off");
    assert_eq!(desktop["locale"], "de", "local members stay");
    assert_eq!(
        desktop["settings_window"]["width"], 1,
        "this machine's window stays"
    );

    // A file changed between the plan and the write is not overwritten.
    let plan = super::plan(&bundle, &Tree::new().locations(), None, &none).unwrap();
    let racing = plan
        .items
        .iter()
        .find(|i| i.path == "config/cuotai.toml")
        .unwrap();
    std::fs::create_dir_all(racing.target_path.parent().unwrap()).unwrap();
    std::fs::write(&racing.target_path, "accounts = []\n").unwrap();
    assert!(matches!(
        plan.apply(|_| true),
        Err(BundleError::Conflict { .. })
    ));
}

#[test]
fn next_steps_follow_the_needs_one_for_one() {
    let source = Tree::full();
    let bundle = collect(&source.locations(), at(), "9.9.9").unwrap();
    let none = |_: &str| None;
    let plan = plan(&bundle, &Tree::new().locations(), None, &none).unwrap();
    assert_eq!(
        plan.next_steps,
        [
            NextStep {
                kind: "env",
                account: 0,
                provider: "groq".into()
            },
            NextStep {
                kind: "sign_in",
                account: 1,
                provider: "claude".into()
            },
        ]
    );
    let json = serde_json::to_value(&plan).unwrap();
    assert_eq!(json["next_steps"][1]["kind"], "sign_in");
}

fn history_metric(used: f64) -> crate::Metric {
    crate::Metric {
        label: "Requests".into(),
        used: Some(used),
        limit: Some(10.0),
        remaining: None,
        unit: "req".into(),
        kind: crate::MetricKind::Quota,
        window: Some(crate::MetricWindow::Daily),
        period: None,
        resets_at: None,
        billing: Default::default(),
        scope: None,
        derived_from: None,
    }
}

#[test]
fn the_history_travels_by_account_and_merges_on_restore() {
    let source = Tree::full();
    let locations = source.locations();
    let config = config_file::parse(TOML, ConfigParseMode::Tolerant)
        .unwrap()
        .config;
    // The fixture's placeholder is not a database.
    std::fs::remove_file(source.at("data/history/history.sqlite3")).unwrap();
    let store = history_store(&locations).unwrap();
    let now = Utc::now();
    for hours in [3, 2] {
        store
            .record(
                &config.accounts[0],
                now - chrono::TimeDelta::hours(hours),
                &[history_metric(hours as f64)],
            )
            .unwrap();
    }
    let bundle = collect_with(
        &locations,
        at(),
        "9.9.9",
        Some(HistorySource {
            store: &store,
            accounts: &config.accounts,
            since: now - chrono::TimeDelta::days(HISTORY_DAYS),
        }),
    )
    .unwrap();
    // Only the account with samples has a file, named by its id.
    let history: Vec<_> = bundle
        .manifest
        .files
        .iter()
        .filter(|e| e.kind == "history")
        .map(|e| e.path.as_str())
        .collect();
    assert_eq!(history, ["history/acct-one.csv"]);
    assert!(bundle.history_bytes() > 0 && bundle.history_bytes() < HISTORY_WARN_BYTES);
    let bundle = Bundle::from_zip(&bundle.to_zip()).unwrap();

    let fresh = Tree::new();
    let none = |_: &str| None;
    let plan = plan(&bundle, &fresh.locations(), Some(Part::History), &none).unwrap();
    assert_eq!(plan.items.len(), 1);
    let item = &plan.items[0];
    assert_eq!(
        (item.action, item.target.as_str()),
        (Action::Merge, "history")
    );
    assert_eq!(item.merge, Some(ImportCount { rows: 2, new: 2 }));
    assert!(plan.needs.is_empty(), "--only history asks for nothing");
    assert_eq!(plan.apply(|_| panic!("a merge asks nothing")).unwrap(), 1);
    let again = super::plan(&bundle, &fresh.locations(), Some(Part::History), &none).unwrap();
    assert_eq!(again.items[0].action, Action::Keep);
    assert_eq!(again.items[0].merge, Some(ImportCount { rows: 2, new: 0 }));
    let merged = history_store(&fresh.locations()).unwrap();
    let csv = merged
        .export_account_csv(&config.accounts[0], now - chrono::TimeDelta::days(1))
        .unwrap()
        .unwrap();
    assert_eq!(String::from_utf8(csv).unwrap().lines().count(), 3);
}

#[test]
fn history_and_label_paths_are_checked() {
    for path in [
        "history/../x.csv",
        "history/UPPER.csv",
        "history/acct-one.txt",
        "history/a/b.csv",
        "prefs/cost-labels.txt",
    ] {
        assert!(!allowed(path), "{path}");
    }
    assert!(allowed("history/acct-one.csv"));
    assert!(allowed("prefs/cost-labels.json"));
    assert_eq!(Part::parse("history"), Some(Part::History));
}

#[test]
fn channel_definitions_travel_without_their_address_and_merge_by_id() {
    let source = Tree::new();
    source.write("config/cuotai.toml", "");
    source.write(
        "config/desktop.json",
        r#"{"channels":[{"id":"aaaa000000000001","kind":"ntfy","label":"Phone","host":"ntfy.example.com","created_at":"2026-11-01T00:00:00Z"}]}"#,
    );
    let bundle = Bundle::from_zip(
        &collect(&source.locations(), at(), "9.9.9")
            .unwrap()
            .to_zip(),
    )
    .unwrap();
    let carried = String::from_utf8(bundle.files["prefs/desktop.json"].clone()).unwrap();
    assert!(carried.contains("ntfy.example.com") && !carried.contains("https://"));

    let target = Tree::new();
    target.write(
        "config/desktop.json",
        r#"{"channels":[{"id":"bbbb000000000002","kind":"slack","label":"Team","host":"hooks.slack.com","created_at":"2026-11-02T00:00:00Z"}]}"#,
    );
    let none = |_: &str| None;
    let plan = plan(&bundle, &target.locations(), Some(Part::Prefs), &none).unwrap();
    let needs: Vec<_> = plan.needs.iter().map(|n| n.resolve("en-US")).collect();
    assert_eq!(
        needs,
        ["Type the address of the notification channel Phone (ntfy) again."]
    );
    assert_eq!(plan.next_steps[0].kind, "channel");
    plan.apply(|_| true).unwrap();
    let merged: serde_json::Value =
        serde_json::from_slice(&std::fs::read(target.at("config/desktop.json")).unwrap()).unwrap();
    let ids: Vec<_> = merged["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["bbbb000000000002", "aaaa000000000001"],
        "local first, none lost"
    );
}

#[test]
fn an_explicit_configuration_keeps_away_from_this_computers_other_files() {
    use crate::config_file::{PathSource, ResolvedPath};
    let tree = Tree::full();
    let resolved = |source| ResolvedPath {
        path: tree.at("config/cuotai.toml"),
        source,
    };
    for source in [PathSource::Argument, PathSource::Env] {
        let isolated = Locations::for_cli_with(resolved(source), None, None, |_| {
            panic!("the process folders are never consulted")
        });
        assert_eq!(isolated.config, tree.at("config/cuotai.toml"));
        assert!(isolated.desktop_settings.is_none(), "{source:?}");
        assert!(isolated.user_dir.is_none(), "{source:?}");
        assert!(isolated.data_root.is_none(), "{source:?}");
        // The variables name the folders explicitly.
        let named = Locations::for_cli_with(
            resolved(source),
            Some(tree.at("user")),
            Some(Some(tree.at("data"))),
            |_| panic!("never consulted"),
        );
        assert_eq!(named.user_dir, Some(tree.at("user")));
        assert_eq!(named.data_root, Some(tree.at("data")));
        assert!(named.desktop_settings.is_none());
    }
    for source in [PathSource::Local, PathSource::System] {
        let found = Locations::for_cli_with(resolved(source), None, None, |config| Locations {
            config,
            desktop_settings: Some(PathBuf::from("desktop.json")),
            user_dir: None,
            data_root: None,
        });
        assert_eq!(found.desktop_settings, Some(PathBuf::from("desktop.json")));
    }

    // A backup of an isolated configuration carries the TOML and its hooks
    // only; its restore lists the rest as left out and writes none of it.
    let isolated = Locations {
        desktop_settings: None,
        user_dir: None,
        data_root: None,
        ..tree.locations()
    };
    let only_config = collect(&isolated, at(), "9.9.9").unwrap();
    assert_eq!(only_config.files.len(), 2);
    let bundle = collect(&tree.locations(), at(), "9.9.9").unwrap();
    let fresh = Tree::new();
    let target = Locations {
        desktop_settings: None,
        user_dir: None,
        data_root: None,
        ..fresh.locations()
    };
    let none = |_: &str| None;
    let plan = plan(&bundle, &target, None, &none).unwrap();
    assert_eq!(
        plan.unplaced,
        [
            "prefs/cost-settings.json",
            "prefs/desktop.json",
            "prefs/pricing.json",
            "user/locales/xx.json",
            "user/themes/night.css",
        ]
    );
    assert_eq!(plan.apply(|_| true).unwrap(), 2);
    assert!(!fresh.at("config/desktop.json").exists());
    assert!(!fresh.at("user").exists());
    assert!(!fresh.at("data").exists());
}

#[test]
fn machine_members_of_the_desktop_settings_neither_travel_nor_restore() {
    let source = Tree::new();
    source.write("config/cuotai.toml", "");
    source.write(
        "config/desktop.json",
        r#"{"motion":"off","data_dir":"/Users/someone/cuotai-data","main_window":{"x":1,"y":2,"width":800,"height":600},"settings_window":{"width":900},"user_dir":"/Users/someone/themes"}"#,
    );
    let bundle = collect(&source.locations(), at(), "9.9.9").unwrap();
    let carried: serde_json::Value =
        serde_json::from_slice(&bundle.files["prefs/desktop.json"]).unwrap();
    assert_eq!(
        carried,
        serde_json::json!({"motion": "off", "user_dir": "@user"}),
        "no geometry, no data folder, the path as a marker"
    );

    // A crafted backup naming real paths and geometry changes none of them.
    let crafted = br#"{"motion":"off","data_dir":"/tmp/elsewhere","config_file":"/tmp/other.toml","user_dir":"/tmp/themes","main_window":{"x":9},"migrations":{"done":true}}"#;
    let local = br#"{"motion":"system","data_dir":"/here/data","main_window":{"x":1}}"#;
    let merged: serde_json::Value =
        serde_json::from_slice(&merge_desktop(Some(local), crafted).unwrap()).unwrap();
    assert_eq!(
        merged,
        serde_json::json!({"motion": "off", "data_dir": "/here/data", "main_window": {"x": 1}})
    );
    let fresh: serde_json::Value =
        serde_json::from_slice(&merge_desktop(None, crafted).unwrap()).unwrap();
    assert_eq!(fresh, serde_json::json!({"motion": "off"}));
}

#[test]
fn history_restored_on_a_fresh_computer_follows_the_ids_the_app_derives() {
    // No explicit ids: they derive from the configuration's identity.
    const NO_IDS: &str =
        "[[accounts]]\nname = \"Groq\"\nprovider = \"groq\"\ncredential_env = \"GROQ_WORK\"\n";
    let source = Tree::new();
    source.write("config/cuotai.toml", NO_IDS);
    let locations = source.locations();
    let identity = ConfigIdentity::of_path(&locations.config).unwrap();
    let mut config = config_file::parse(NO_IDS, ConfigParseMode::Tolerant)
        .unwrap()
        .config;
    config.resolve_ids(Some(&identity));
    let store = history_store(&locations).unwrap();
    let now = Utc::now();
    store
        .record(
            &config.accounts[0],
            now - chrono::TimeDelta::hours(1),
            &[history_metric(1.0)],
        )
        .unwrap();
    let bundle = collect_with(
        &locations,
        at(),
        "9.9.9",
        Some(HistorySource {
            store: &store,
            accounts: &config.accounts,
            since: now - chrono::TimeDelta::days(HISTORY_DAYS),
        }),
    )
    .unwrap();

    // The configuration's folder does not exist yet on the new computer.
    let fresh = Tree::new();
    let target = Locations {
        config: fresh.at("new/deep/cuotai.toml"),
        ..fresh.locations()
    };
    let none = |_: &str| None;
    let plan = plan(&bundle, &target, None, &none).unwrap();
    plan.apply(|_| true).unwrap();

    // What the app reads once the file exists.
    let identity = ConfigIdentity::of_path(&target.config).unwrap();
    let mut restored = config_file::parse(
        &std::fs::read_to_string(&target.config).unwrap(),
        ConfigParseMode::Tolerant,
    )
    .unwrap()
    .config;
    restored.resolve_ids(Some(&identity));
    let app = HistoryStore::new(fresh.at("data/history/history.sqlite3"), &identity);
    assert!(
        app.latest(&restored.accounts[0]).unwrap().is_some(),
        "the samples are under the id the app gives the account"
    );
}
