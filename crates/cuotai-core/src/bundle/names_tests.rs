//! PRD 0046 re-validation G-0046-3: names, manifests, dates, channel
//! labels, variable names and links a crafted backup could abuse.

use super::*;

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn locations(root: &Path) -> Locations {
    Locations {
        config: root.join("config/cuotai.toml"),
        desktop_settings: Some(root.join("config/desktop.json")),
        user_dir: Some(root.join("user")),
        data_root: Some(root.join("data")),
    }
}

fn past() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

/// A small valid bundle: one accounts file and one theme.
fn source() -> Bundle {
    let root = dir();
    write(root.path(), "config/cuotai.toml", "accounts = []\n");
    write(root.path(), "user/themes/night.css", ":root{}");
    collect(&locations(root.path()), past(), "9.9.9").unwrap()
}

fn with_file(bundle: &Bundle, path: &str) -> Bundle {
    let mut bundle = bundle.clone();
    bundle.files.insert(path.into(), b"x".to_vec());
    bundle.manifest.files.push(Entry {
        path: path.into(),
        sha256: sha256(b"x"),
        kind: kind_of(path).into(),
    });
    bundle
}

fn reread(bundle: &Bundle) -> Result<Bundle, BundleError> {
    Bundle::from_zip(&bundle.to_zip())
}

#[test]
fn crafted_names_and_manifests_are_refused() {
    let bundle = source();
    assert!(reread(&bundle).is_ok());
    for name in [
        "user/themes/a\u{1b}[2J.css",
        "user/themes/a\u{202e}ssc.css",
        "user/themes/a\u{200f}b.css",
        "user/themes/run.sh",
        "user/themes/a:b.css",
        "user/themes/a?.css",
        "user/locales/CON",
        "user/locales/nul.json",
        "user/locales/com1.json",
        "user/locales/Lpt9.json",
        "user/themes/x.css.",
        "user/themes/x.css ",
        "user/locales/theme.css",
    ] {
        assert_eq!(
            reread(&with_file(&bundle, name)).unwrap_err(),
            BundleError::Invalid,
            "refused: {}",
            name.escape_default()
        );
    }
    // Names that only resemble a reserved one stay valid.
    for name in ["user/locales/console.json", "user/locales/com10.json"] {
        assert!(reread(&with_file(&bundle, name)).is_ok(), "{name}");
    }

    let case = with_file(&bundle, "user/themes/Night.css");
    assert_eq!(reread(&case).unwrap_err(), BundleError::Invalid);

    let mut older = bundle.clone();
    older.manifest.schema = 0;
    assert_eq!(reread(&older).unwrap_err(), BundleError::Invalid);
    assert_eq!(reread(&older).unwrap_err().exit_code(), 2);

    let mut undated = bundle.clone();
    undated.manifest.created_at = "yesterday".into();
    assert_eq!(reread(&undated).unwrap_err(), BundleError::Invalid);

    let mut escaped = bundle.clone();
    escaped.manifest.cuotai_version = "1.0\u{1b}]0;x\u{7}".into();
    assert_eq!(reread(&escaped).unwrap_err(), BundleError::Invalid);

    let mut empty = bundle.clone();
    empty.manifest.cuotai_version = String::new();
    assert_eq!(reread(&empty).unwrap_err(), BundleError::Invalid);

    // A development build's version is what a release writes too.
    let mut dev = bundle;
    dev.manifest.cuotai_version = "0.80.0 (dev abc1234, modified)".into();
    assert!(reread(&dev).is_ok());
}

#[test]
fn an_undated_or_future_backup_never_pre_checks_a_replacement() {
    let source = dir();
    write(source.path(), "config/cuotai.toml", "accounts = []\n");
    write(
        source.path(),
        "config/hooks.toml",
        "[[hooks]]\nname = \"a\"\n",
    );
    let target = dir();
    write(
        target.path(),
        "config/hooks.toml",
        "[[hooks]]\nname = \"b\"\n",
    );
    let none = |_: &str| None;

    // The local file is older than a past backup: a plain replacement.
    let past_bundle = collect(&locations(source.path()), past(), "9.9.9").unwrap();
    let hooks_action = |bundle: &Bundle| {
        let plan = plan(bundle, &locations(target.path()), None, &none).unwrap();
        plan.items
            .iter()
            .find(|item| item.path == "config/hooks.toml")
            .unwrap()
            .action
    };
    let future = Utc::now() + chrono::TimeDelta::hours(1);
    let future_bundle = collect(&locations(source.path()), future, "9.9.9").unwrap();
    assert_eq!(hooks_action(&future_bundle), Action::Skip);

    // A date that does not parse (a bundle built in memory) is unknown too.
    let mut undated = past_bundle.clone();
    undated.manifest.created_at = "yesterday".into();
    assert_eq!(hooks_action(&undated), Action::Skip);

    // A trustworthy date still tells an older local file from a newer one.
    let modified = std::fs::metadata(target.path().join("config/hooks.toml"))
        .unwrap()
        .modified()
        .unwrap();
    let after = DateTime::<Utc>::from(modified) + chrono::TimeDelta::seconds(2);
    let after_bundle = collect(&locations(source.path()), after, "9.9.9").unwrap();
    assert_eq!(hooks_action(&after_bundle), Action::Replace);
}

#[test]
fn channel_needs_skip_malformed_and_disguised_labels() {
    let channel = |id: &str, label: &str| {
        serde_json::json!({
            "id": id,
            "kind": "ntfy",
            "label": label,
            "host": "ntfy.example.com",
            "created_at": "2026-11-01T00:00:00Z",
        })
    };
    let desktop = serde_json::json!({
        "channels": [
            channel("aaaa000000000001", "Pho\u{202e}ne"),
            channel("aaaa000000000002", "Ph\u{1b}[2Jone"),
            channel("aaaa000000000003", "Phone \u{2066}x"),
            {"id": "aaaa000000000004", "kind": "ntfy", "label": "No date", "host": "ntfy.example.com"},
            channel("not-hex", "Bad id"),
            channel("aaaa000000000006", "Phone"),
        ]
    });
    let needs = channel_needs(&serde_json::to_vec(&desktop).unwrap());
    assert_eq!(needs.len(), 1);
    assert_eq!(
        needs[0].0.resolve("en-US"),
        "Type the address of the notification channel Phone (ntfy) again."
    );
    assert_eq!(
        needs[0].1,
        NextStep {
            kind: "channel",
            account: 5,
            provider: "ntfy".into()
        },
        "the position in the desktop settings"
    );
}

#[test]
fn validate_label_refuses_bidi_overrides_and_isolates() {
    use crate::channels::validate_label;
    assert!(validate_label("Phone").is_ok());
    assert!(validate_label("Pho\u{202e}ne").is_err());
    assert!(validate_label("Pho\u{2066}ne").is_err());
    assert!(validate_label("Pho\u{1b}ne").is_err());
}

#[test]
fn usable_env_name_table() {
    use crate::account_edit::{default_env, usable_env_name};
    for name in ["GROQ_API_KEY", "GROQ_API_KEY_WORK", "my_key_2", "_X"] {
        assert!(usable_env_name(name), "{name}");
    }
    for provider in crate::Provider::ALL {
        let name = default_env(provider);
        assert!(name.is_empty() || usable_env_name(name), "{name}");
    }
    let long = "A".repeat(129);
    for name in [
        "sk-FAKE-0047-pasted-value".to_owned(),
        format!("gsk_FAKE0047{}", "a1".repeat(24)),
        "AKIAFAKE0047FAKE0047".to_owned(),
        "ghp_FAKE0047FAKE0047FAKE".to_owned(),
        long,
        "A B".to_owned(),
        "abcDEF1234567890abcdef12".to_owned(),
        "A\u{1b}[1AB".to_owned(),
        String::new(),
    ] {
        assert!(!usable_env_name(&name), "refused: {}", name.len());
    }
}

#[test]
fn needs_never_show_an_invalid_variable_name() {
    let config = "[[accounts]]\nname = \"a\"\nprovider = \"groq\"\ncredential_env = \"A\\u001b[1AB\"\n\n[[accounts]]\nname = \"b\"\nprovider = \"groq\"\ncredential_env = \"gsk_FAKE0047a1a1a1a1a1a1a1a1a1a1a1a1a1a1\"\n\n[[accounts]]\nname = \"c\"\nprovider = \"groq\"\ncredential_env = \"GROQ_WORK\"\n";
    let none = |_: &str| None;
    let needs = needs(config.as_bytes(), &none);
    let texts: Vec<_> = needs
        .iter()
        .map(|(text, _)| text.resolve("en-US"))
        .collect();
    assert_eq!(
        texts,
        [
            "Fix the variable name of account 1 (groq); it is not valid.",
            "Fix the variable name of account 2 (groq); it is not valid.",
            "Set GROQ_WORK for account 3 (groq).",
        ]
    );
    assert_eq!(needs[0].0.id, "bundle.need.env_invalid");
    assert!(needs[..2]
        .iter()
        .all(|(text, step)| step.kind == "env" && !text.params.contains_key("variable")));
    let json = serde_json::to_string(&needs.iter().map(|(t, _)| t).collect::<Vec<_>>()).unwrap();
    assert!(!json.contains("FAKE") && !json.contains("\\u001b"));
}

#[cfg(unix)]
#[test]
fn a_restore_writes_through_a_linked_accounts_file() {
    let source = dir();
    write(source.path(), "config/cuotai.toml", "accounts = []\n");
    write(
        source.path(),
        "data/cost/pricing.json",
        r#"{"currency":"USD"}"#,
    );
    let bundle = collect(&locations(source.path()), past(), "9.9.9").unwrap();

    let target = dir();
    write(target.path(), "dotfiles/cuotai.toml", "# mine\n");
    std::fs::create_dir_all(target.path().join("config")).unwrap();
    let link = target.path().join("config/cuotai.toml");
    std::os::unix::fs::symlink(target.path().join("dotfiles/cuotai.toml"), &link).unwrap();
    let none = |_: &str| None;
    let plan = plan(
        &bundle,
        &locations(target.path()),
        Some(Part::Config),
        &none,
    )
    .unwrap();
    let item = plan
        .items
        .iter()
        .find(|item| item.path == "config/cuotai.toml")
        .unwrap();
    assert_eq!(
        item.target,
        home_relative(&link),
        "the path the person knows"
    );
    plan.apply(true, |_| true).unwrap();
    assert!(std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        std::fs::read_to_string(target.path().join("dotfiles/cuotai.toml")).unwrap(),
        "accounts = []\n"
    );

    // A linked data-folder preference is refused, and nothing is written.
    let other = dir();
    write(other.path(), "elsewhere.json", "{}");
    std::fs::create_dir_all(other.path().join("data/cost")).unwrap();
    std::os::unix::fs::symlink(
        other.path().join("elsewhere.json"),
        other.path().join("data/cost/pricing.json"),
    )
    .unwrap();
    let refused = plan_or_error(&bundle, other.path());
    assert!(matches!(refused, Err(BundleError::Io { .. })));
    assert!(!other.path().join("config/cuotai.toml").exists());
    assert_eq!(
        std::fs::read_to_string(other.path().join("elsewhere.json")).unwrap(),
        "{}"
    );
}

#[cfg(unix)]
fn plan_or_error(bundle: &Bundle, root: &Path) -> Result<Plan, BundleError> {
    let none = |_: &str| None;
    plan(bundle, &locations(root), None, &none)
}
