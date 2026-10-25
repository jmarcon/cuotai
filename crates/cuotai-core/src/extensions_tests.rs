//! PRD 0017 RF1: discovery of extension manifests in fixture folders (valid
//! manifests, unknown fields, the interval minimum, commands outside the
//! folder, duplicate ids) and their catalog entries.

use super::*;
use std::collections::BTreeSet;
use std::fs;

const VALID: &str = r#"
id = "proxy"
name = "Proxy balance"
version = "1.2.0"
schema = 1
command = ["./run.sh", "--json"]
interval_seconds = 300
description = "Balance of my LLM proxy."
homepage = "https://example.com/proxy"
env = ["PROXY_TOKEN", "PROXY_URL", "PROXY_TOKEN"]
"#;

/// `VALID` with `id` and the line `key = …` replaced (or added), or removed
/// when `value` is `None`.
fn with(id: &str, key: &str, value: Option<&str>) -> String {
    let mut lines: Vec<String> = VALID
        .lines()
        .filter(|line| !line.starts_with(&format!("{key} =")))
        .map(|line| {
            if line.starts_with("id =") {
                format!("id = \"{id}\"")
            } else {
                line.to_owned()
            }
        })
        .collect();
    if let Some(value) = value {
        lines.push(format!("{key} = {value}"));
    }
    lines.join("\n")
}

/// A folder `name` under `root` with `manifest` and an executable `run.sh`.
fn extension(root: &Path, name: &str, manifest: &str) -> PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
    script(&dir.join("run.sh"));
    dir
}

fn script(path: &Path) {
    fs::write(path, "#!/bin/sh\necho '{}'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// The reason id of the only rejection.
fn rejection(root: &Path) -> String {
    let discovery = discover(root);
    assert!(discovery.extensions.is_empty(), "{discovery:?}");
    assert_eq!(discovery.rejected.len(), 1, "{discovery:?}");
    discovery.rejected[0].reason.id.to_string()
}

/// RF5.19: a program the operating system would hand to `cmd.exe` or
/// PowerShell is refused in every spelling, because those re-parse the exact
/// argument vector this contract fixes. A directly executed file (`.exe`,
/// or a Unix script with the executable bit) is still accepted.
#[test]
fn an_interpreted_wrapper_is_refused_and_a_direct_executable_is_not() {
    let root = tempfile::tempdir().unwrap();
    for name in [
        "quota.cmd",
        "quota.CMD",
        "quota.bat",
        "quota.Bat",
        "quota.ps1",
        "quota.vbs",
        "quota.com",
    ] {
        let dir = root.path().join("proxy");
        fs::create_dir_all(&dir).unwrap();
        let manifest = VALID.replace("\"./run.sh\", \"--json\"", &format!("\"./{name}\""));
        fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
        script(&dir.join(name));
        assert_eq!(
            rejection(root.path()),
            "extensions.error.interpreted",
            "{name} must be refused"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    let dir = extension(root.path(), "proxy", VALID);
    script(&dir.join("quota.exe"));
    let discovery = discover(root.path());
    assert!(discovery.rejected.is_empty(), "{:?}", discovery.rejected);
    assert!(discovery.get("proxy").is_some());
}

/// The suffix check runs on the **resolved** program: a symlink named
/// `run.exe` pointing at `script.cmd` would be handed to `cmd.exe` with our
/// exact argv, so it is refused too.
#[cfg(unix)]
#[test]
fn a_symlink_to_a_wrapper_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("proxy");
    fs::create_dir_all(&dir).unwrap();
    let manifest = VALID.replace("\"./run.sh\", \"--json\"", "\"./run.exe\"");
    fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
    script(&dir.join("script.cmd"));
    std::os::unix::fs::symlink(dir.join("script.cmd"), dir.join("run.exe")).unwrap();
    assert_eq!(rejection(root.path()), "extensions.error.interpreted");
}

#[test]
fn a_valid_manifest_is_discovered_with_its_fields_and_defaults() {
    let root = tempfile::tempdir().unwrap();
    let dir = extension(root.path(), "proxy", VALID);
    let discovery = discover(root.path());
    assert!(discovery.rejected.is_empty(), "{:?}", discovery.rejected);
    let manifest = discovery.get("proxy").expect("discovered");
    let dir = dir.canonicalize().unwrap();
    assert_eq!(manifest.name, "Proxy balance");
    assert_eq!(manifest.version, "1.2.0");
    assert_eq!(manifest.schema, 1);
    assert_eq!(manifest.command, ["./run.sh", "--json"]);
    assert_eq!(manifest.interval_seconds, 300);
    assert_eq!(manifest.timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
    assert_eq!(
        manifest.description.as_deref(),
        Some("Balance of my LLM proxy.")
    );
    assert_eq!(
        manifest.homepage.as_deref(),
        Some("https://example.com/proxy")
    );
    assert_eq!(manifest.env, ["PROXY_TOKEN", "PROXY_URL"]);
    assert_eq!(manifest.program, dir.join("run.sh"));
    assert_eq!(manifest.dir, dir);
}

#[test]
fn optional_fields_may_be_absent_and_the_program_may_sit_in_a_subfolder() {
    let root = tempfile::tempdir().unwrap();
    let text = "id = \"min\"\nname = \"Min\"\nversion = \"0\"\nschema = 1\n\
                command = [\"bin/fetch\"]\ninterval_seconds = 60\ntimeout_seconds = 60\n";
    let dir = root.path().join("min");
    fs::create_dir_all(dir.join("bin")).unwrap();
    fs::write(dir.join(MANIFEST_FILE), text).unwrap();
    script(&dir.join("bin/fetch"));
    let discovery = discover(root.path());
    let manifest = discovery.get("min").expect("discovered");
    assert_eq!(manifest.interval_seconds, MIN_INTERVAL_SECONDS);
    assert_eq!(manifest.timeout_seconds, MAX_TIMEOUT_SECONDS);
    assert_eq!(manifest.description, None);
    assert_eq!(manifest.homepage, None);
    assert!(manifest.env.is_empty());
    assert!(manifest.program.ends_with(Path::new("bin").join("fetch")));
}

#[test]
fn a_missing_directory_has_no_extensions_and_hidden_folders_and_files_are_skipped() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(discover(&root.path().join("absent")), Discovery::default());
    extension(root.path(), ".git", "not a manifest");
    fs::write(root.path().join("README.md"), "notes").unwrap();
    assert_eq!(discover(root.path()), Discovery::default());
}

#[test]
fn an_invalid_manifest_is_rejected_without_hiding_the_valid_ones() {
    let root = tempfile::tempdir().unwrap();
    extension(
        root.path(),
        "a-good",
        &with("good", "name", Some("\"Good\"")),
    );
    extension(
        root.path(),
        "b-broken",
        "id = \"broken\"\nschema = 1\nname = [",
    );
    fs::create_dir_all(root.path().join("c-empty")).unwrap();
    extension(
        root.path(),
        "d-other",
        &with("other", "name", Some("\"Other\"")),
    );
    let discovery = discover(root.path());
    let ids: Vec<&str> = discovery.extensions.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["good", "other"]);
    let rejected: Vec<(&Path, &str)> = discovery
        .rejected
        .iter()
        .map(|r| (r.dir.strip_prefix(root.path()).unwrap(), &*r.reason.id))
        .collect();
    assert_eq!(
        rejected,
        [
            (Path::new("b-broken"), "extensions.error.toml"),
            (Path::new("c-empty"), "extensions.error.no_manifest"),
        ]
    );
    assert_eq!(discovery.rejected[0].reason.params["line"], 3usize.into());
}

#[test]
fn an_unknown_field_is_rejected_by_name() {
    let root = tempfile::tempdir().unwrap();
    extension(root.path(), "proxy", &with("proxy", "shell", Some("true")));
    let discovery = discover(root.path());
    let rejected = &discovery.rejected[0];
    assert!(discovery.extensions.is_empty());
    assert_eq!(rejected.reason.id, "extensions.error.unknown_field");
    assert_eq!(rejected.reason.params["field"], "\"shell\"".into());
    assert_eq!(rejected.id.as_deref(), Some("proxy"));
}

#[test]
fn the_interval_has_a_minimum_of_sixty_seconds() {
    for (value, accepted) in [("59", false), ("0", false), ("-60", false), ("60", true)] {
        let root = tempfile::tempdir().unwrap();
        extension(
            root.path(),
            "p",
            &with("p", "interval_seconds", Some(value)),
        );
        let discovery = discover(root.path());
        assert_eq!(discovery.get("p").is_some(), accepted, "{value}");
        if !accepted {
            assert_eq!(
                rejection(root.path()),
                "extensions.error.interval",
                "{value}"
            );
        }
    }
    let root = tempfile::tempdir().unwrap();
    extension(root.path(), "p", &with("p", "interval_seconds", None));
    assert_eq!(rejection(root.path()), "extensions.error.toml");
}

#[test]
fn the_timeout_stays_between_one_and_sixty_seconds() {
    for (value, accepted) in [("0", false), ("61", false), ("1", true), ("60", true)] {
        let root = tempfile::tempdir().unwrap();
        extension(root.path(), "p", &with("p", "timeout_seconds", Some(value)));
        assert_eq!(
            discover(root.path()).get("p").is_some(),
            accepted,
            "{value}"
        );
    }
}

#[test]
fn a_command_outside_the_extension_folder_is_rejected() {
    // `OUTSIDE` stands for the absolute path of a script next to the
    // extensions directory.
    for (command, reason) in [
        ("[OUTSIDE]", "extensions.error.command_outside"),
        ("[\"../outside.sh\"]", "extensions.error.command_outside"),
        (
            "[\"./sub/../../outside.sh\"]",
            "extensions.error.command_outside",
        ),
        ("[\".\"]", "extensions.error.command_outside"),
        (
            "[\"sh\", \"-c\", \"echo\"]",
            "extensions.error.program_missing",
        ),
        ("[]", "extensions.error.command"),
        ("[\"\"]", "extensions.error.command"),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside.sh");
        script(&outside);
        let extensions = tmp.path().join("extensions");
        let command = command.replace("OUTSIDE", &quoted(&outside.display().to_string()));
        extension(&extensions, "p", &with("p", "command", Some(&command)));
        assert_eq!(rejection(&extensions), reason, "{command}");
    }
}

#[cfg(unix)]
#[test]
fn a_link_that_leads_out_of_the_folder_is_rejected_and_one_inside_is_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let extensions = tmp.path().join("extensions");
    script(&tmp.path().join("outside.sh"));
    let dir = extension(
        &extensions,
        "p",
        &with("p", "command", Some("[\"./link\"]")),
    );
    std::os::unix::fs::symlink(tmp.path().join("outside.sh"), dir.join("link")).unwrap();
    assert_eq!(rejection(&extensions), "extensions.error.command_outside");

    fs::remove_file(dir.join("link")).unwrap();
    std::os::unix::fs::symlink(dir.join("run.sh"), dir.join("link")).unwrap();
    let discovery = discover(&extensions);
    let manifest = discovery.get("p").expect("a link inside the folder");
    assert_eq!(manifest.program, dir.canonicalize().unwrap().join("run.sh"));
}

#[cfg(unix)]
#[test]
fn a_program_that_is_not_executable_is_rejected() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let dir = extension(root.path(), "p", &with("p", "name", Some("\"P\"")));
    fs::set_permissions(dir.join("run.sh"), fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(rejection(root.path()), "extensions.error.not_executable");
}

#[test]
fn duplicate_ids_reject_every_folder_that_shares_them() {
    let root = tempfile::tempdir().unwrap();
    extension(
        root.path(),
        "first",
        &with("same", "name", Some("\"First\"")),
    );
    extension(
        root.path(),
        "second",
        &with("same", "name", Some("\"Second\"")),
    );
    extension(
        root.path(),
        "third",
        &with("unique", "name", Some("\"Third\"")),
    );
    let discovery = discover(root.path());
    let ids: Vec<&str> = discovery.extensions.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["unique"]);
    assert_eq!(discovery.rejected.len(), 2);
    for (rejected, folder) in discovery.rejected.iter().zip(["first", "second"]) {
        assert_eq!(rejected.dir, root.path().join(folder));
        assert_eq!(rejected.id.as_deref(), Some("same"));
        assert_eq!(rejected.reason.id, "extensions.error.duplicate_id");
    }
}

#[test]
fn the_schema_is_checked_before_the_fields_of_a_newer_contract() {
    let root = tempfile::tempdir().unwrap();
    let newer = with("p", "schema", Some("2")) + "\nsandbox = \"strict\"\n";
    extension(root.path(), "p", &newer);
    assert_eq!(rejection(root.path()), "extensions.error.schema");

    let root = tempfile::tempdir().unwrap();
    extension(root.path(), "p", &with("p", "schema", None));
    assert_eq!(rejection(root.path()), "extensions.error.schema_missing");
}

/// PRD 0028 RF7.5: a manifest that needs a newer Cuotai is refused with the
/// required version, before any other validation error (here a newer
/// `schema` and an unknown field); an older or equal one loads.
#[test]
fn min_cuotai_is_checked_before_any_other_error() {
    let root = tempfile::tempdir().unwrap();
    let newer =
        with("p", "schema", Some("2")) + "\nsandbox = \"strict\"\nmin_cuotai = \"999.0.0\"\n";
    extension(root.path(), "p", &newer);
    let discovery = discover(root.path());
    let reason = &discovery.rejected[0].reason;
    assert_eq!(reason.id, "extensions.error.newer_cuotai");
    assert_eq!(reason.params["required"], "999.0.0".into());
    assert_eq!(reason.params["current"], env!("CARGO_PKG_VERSION").into());

    let own = format!("\"{}\"", env!("CARGO_PKG_VERSION"));
    for (value, accepted) in [
        ("\"0.1.0\"", true),
        (own.as_str(), true),
        ("\"0.1\"", false),
        ("\"latest\"", false),
        ("81", false),
    ] {
        let root = tempfile::tempdir().unwrap();
        extension(root.path(), "p", &with("p", "min_cuotai", Some(value)));
        if accepted {
            let discovery = discover(root.path());
            assert_eq!(discovery.extensions.len(), 1, "{value}: {discovery:?}");
        } else {
            assert_eq!(
                rejection(root.path()),
                "extensions.error.min_cuotai",
                "{value}"
            );
        }
    }
}

/// PRD 0028 RF7.5: top-level `x-*` tables are ignored; an `x-*` key that is
/// not a table and any other unknown key are still refused.
#[test]
fn x_tables_are_ignored_and_other_unknown_keys_are_not() {
    let root = tempfile::tempdir().unwrap();
    let text = with("p", "min_cuotai", None) + "\n[x-vendor]\nchannel = \"beta\"\nlevel = 2\n";
    extension(root.path(), "p", &text);
    let discovery = discover(root.path());
    assert_eq!(discovery.extensions.len(), 1, "{discovery:?}");

    let root = tempfile::tempdir().unwrap();
    extension(root.path(), "p", &with("p", "x-flag", Some("true")));
    assert_eq!(rejection(root.path()), "extensions.error.unknown_field");

    let root = tempfile::tempdir().unwrap();
    let text = with("p", "min_cuotai", None) + "\n[vendor]\nchannel = \"beta\"\n";
    extension(root.path(), "p", &text);
    assert_eq!(rejection(root.path()), "extensions.error.unknown_field");
}

/// PRD 0028 RF7.4: `[[secrets]]` entries are read by name, variable and
/// description; a reserved, repeated or `env`-listed variable, a bad name,
/// an unknown key inside an entry or too many entries are refused.
#[test]
fn secrets_are_declared_by_name_variable_and_description() {
    let secret = |name: &str, env: &str| {
        format!("\n[[secrets]]\nname = \"{name}\"\nenv = \"{env}\"\ndescription = \"Token\"\n")
    };
    let root = tempfile::tempdir().unwrap();
    let text = with("p", "min_cuotai", None) + &secret("token", "P_SECRET");
    extension(root.path(), "p", &text);
    let discovery = discover(root.path());
    let manifest = &discovery.extensions[0];
    assert_eq!(
        manifest.secrets,
        vec![Secret {
            name: "token".into(),
            env: "P_SECRET".into(),
            description: "Token".into()
        }]
    );

    for (case, text) in [
        ("reserved", secret("token", "PATH")),
        ("listed in env", secret("token", "PROXY_TOKEN")),
        ("bad name", secret("Token", "P_SECRET")),
        (
            "repeated",
            secret("a", "P_SECRET") + &secret("b", "P_SECRET"),
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        extension(root.path(), "p", &(with("p", "min_cuotai", None) + &text));
        assert_eq!(rejection(root.path()), "extensions.error.secret", "{case}");
    }
    let many: String = (0..=SECRETS_MAX)
        .map(|i| secret(&format!("s{i}"), &format!("S_{i}")))
        .collect();
    let root = tempfile::tempdir().unwrap();
    extension(root.path(), "p", &(with("p", "min_cuotai", None) + &many));
    assert_eq!(rejection(root.path()), "extensions.error.secrets_count");

    let root = tempfile::tempdir().unwrap();
    let text = with("p", "min_cuotai", None) + &secret("token", "P_SECRET") + "value = \"x\"\n";
    extension(root.path(), "p", &text);
    assert_eq!(rejection(root.path()), "extensions.error.toml");
}

/// PRD 0028 RF7.1: `overrides` names a built-in provider by its slug.
#[test]
fn overrides_names_a_builtin_provider() {
    let root = tempfile::tempdir().unwrap();
    extension(
        root.path(),
        "p",
        &with("p", "overrides", Some("\"claude\"")),
    );
    let discovery = discover(root.path());
    assert_eq!(
        discovery.extensions[0].overrides,
        Some(crate::Provider::Claude)
    );
    for value in ["\"extension\"", "\"nope\"", "\"\""] {
        let root = tempfile::tempdir().unwrap();
        extension(root.path(), "p", &with("p", "overrides", Some(value)));
        assert_eq!(
            rejection(root.path()),
            "extensions.error.overrides",
            "{value}"
        );
    }
}

#[test]
fn each_field_is_validated() {
    for (key, value, reason) in [
        ("id", None, "extensions.error.toml"),
        ("name", Some("\"  \""), "extensions.error.text"),
        ("name", Some("\"a\\u001b[31m\""), "extensions.error.text"),
        (
            "version",
            Some(&*format!("\"{}\"", "9".repeat(33))),
            "extensions.error.text",
        ),
        (
            "description",
            Some("\"bell \\u0007\""),
            "extensions.error.text",
        ),
        (
            "homepage",
            Some("\"ftp://example.com\""),
            "extensions.error.homepage",
        ),
        (
            "homepage",
            Some("\"https://me:secret@example.com\""),
            "extensions.error.homepage",
        ),
        (
            "homepage",
            Some("\"not a url\""),
            "extensions.error.homepage",
        ),
        ("env", Some("[\"1BAD\"]"), "extensions.error.env_name"),
        ("env", Some("[\"A=B\"]"), "extensions.error.env_name"),
        ("command", Some("\"./run.sh\""), "extensions.error.toml"),
    ] {
        let root = tempfile::tempdir().unwrap();
        extension(root.path(), "p", &with("p", key, value));
        assert_eq!(rejection(root.path()), reason, "{key} = {value:?}");
    }
    for id in ["Proxy", "-proxy", "pro xy", "proxy.v2", &"p".repeat(65), ""] {
        let root = tempfile::tempdir().unwrap();
        extension(root.path(), "p", &with(id, "name", Some("\"P\"")));
        assert_eq!(rejection(root.path()), "extensions.error.id", "{id:?}");
    }
    let names: Vec<String> = (0..=ENV_MAX_NAMES).map(|i| format!("\"V{i}\"")).collect();
    let root = tempfile::tempdir().unwrap();
    let env = format!("[{}]", names.join(", "));
    extension(root.path(), "p", &with("p", "env", Some(&env)));
    assert_eq!(rejection(root.path()), "extensions.error.env_count");
}

#[test]
fn an_oversized_manifest_is_refused_unread() {
    let root = tempfile::tempdir().unwrap();
    let padding = format!("\n# {}\n", "x".repeat(MANIFEST_MAX_BYTES as usize));
    extension(
        root.path(),
        "p",
        &(with("p", "name", Some("\"P\"")) + &padding),
    );
    assert_eq!(rejection(root.path()), "extensions.error.too_large");
}

#[cfg(unix)]
#[test]
fn a_manifest_that_is_not_a_regular_file_is_refused_without_blocking() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("p");
    fs::create_dir_all(&dir).unwrap();
    // A FIFO blocks whoever opens it until a writer shows up.
    let made = std::process::Command::new("mkfifo")
        .arg(dir.join(MANIFEST_FILE))
        .status()
        .expect("mkfifo");
    assert!(made.success());
    assert_eq!(rejection(root.path()), "extensions.error.unreadable");
}

#[test]
fn the_catalog_offers_each_valid_extension_with_the_extension_level_and_warning() {
    let root = tempfile::tempdir().unwrap();
    extension(root.path(), "proxy", VALID);
    extension(root.path(), "broken", "schema = 1");
    let catalog = discover(root.path()).catalog();
    assert_eq!(catalog.len(), 1);
    let entry = &catalog[0];
    assert_eq!(entry.id, "proxy");
    assert_eq!(entry.level, Level::Extension);
    assert_eq!(entry.label.text(), "Extension: Proxy balance");
    assert_eq!(entry.label.resolve("en-US"), "Extension: Proxy balance");
    assert_eq!(entry.label.resolve("es"), "Extensión: Proxy balance");
    assert_eq!(
        entry.warning.resolve("en-US"),
        "Runs a local program; review the code before enabling."
    );
    assert_eq!(entry.level.label().resolve("en-US"), "Extension");
    // PRD 0027 RF1.3: its own hint, from the description.
    assert_eq!(entry.hint.resolve("en-US"), "Balance of my LLM proxy.");
    let json = serde_json::to_value(entry).unwrap();
    assert_eq!(json["level"], "extension");
}

#[test]
fn every_rejection_and_catalog_text_is_translated() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    let env: Vec<String> = (0..=ENV_MAX_NAMES).map(|i| format!("\"V{i}\"")).collect();
    let env = format!("[{}]", env.join(", "));
    let oversized = with("big", "name", Some("\"B\"")) + &"#".repeat(MANIFEST_MAX_BYTES as usize);
    fs::create_dir_all(base.join("no-manifest")).unwrap();
    for (folder, text) in [
        ("syntax", "schema = 1\nname = [".to_owned()),
        ("schema", with("schema", "schema", Some("2"))),
        ("schema-missing", with("schema-missing", "schema", None)),
        ("unknown", with("unknown", "shell", Some("true"))),
        ("id", with("Bad Id", "name", Some("\"P\""))),
        ("text", with("text", "name", Some("\"\""))),
        ("interval", with("interval", "interval_seconds", Some("1"))),
        ("timeout", with("timeout", "timeout_seconds", Some("0"))),
        ("homepage", with("homepage", "homepage", Some("\"x\""))),
        ("env-name", with("env-name", "env", Some("[\"1\"]"))),
        ("env-count", with("env-count", "env", Some(&env))),
        ("command", with("command", "command", Some("[]"))),
        ("outside", with("outside", "command", Some("[\"../x\"]"))),
        (
            "missing",
            with("missing", "command", Some("[\"./absent\"]")),
        ),
        ("oversized", oversized),
        ("dup-1", with("dup", "name", Some("\"D\""))),
        ("dup-2", with("dup", "name", Some("\"D\""))),
        ("valid", VALID.to_owned()),
    ] {
        extension(base, folder, &text);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = extension(base, "mode", &with("mode", "name", Some("\"M\"")));
        fs::set_permissions(dir.join("run.sh"), fs::Permissions::from_mode(0o644)).unwrap();
    }
    let discovery = discover(base);
    let entry = discovery.catalog().pop().expect("the valid extension");
    let mut texts = vec![
        entry.label,
        entry.warning,
        entry.level.label(),
        directory_unreadable(base),
        manifest_unreadable(),
    ];
    texts.extend(
        discovery
            .rejected
            .into_iter()
            .map(|rejected| rejected.reason),
    );
    let ids: BTreeSet<&str> = texts.iter().map(|text| &*text.id).collect();
    assert_eq!(ids.len(), if cfg!(unix) { 23 } else { 22 }, "{ids:?}");
    for locale in crate::text::catalog_locales() {
        let catalog = crate::text::core_catalog(locale).unwrap();
        let missing: Vec<_> = ids
            .iter()
            .filter(|id| !catalog.contains_key(**id))
            .collect();
        assert!(missing.is_empty(), "{locale} lacks {missing:?}");
        for text in &texts {
            let resolved = text.resolve(locale);
            assert!(!resolved.contains('{'), "{locale}: {resolved}");
        }
    }
}
