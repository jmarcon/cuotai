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
