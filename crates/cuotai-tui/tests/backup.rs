//! PRD 0046 RF2/RF3: `--backup` and `--restore` from the command line, in
//! sandboxed homes (two "computers"), with no network and no vault.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cuotai-bundle-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(home: &Path, vars: &[(&str, &str)], args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cuotai"));
    command
        .args(["--config", home.join("cuotai.toml").to_str().unwrap()])
        .args(args)
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("CUOTAI_DATA_DIR", home.join("cuotai-data"))
        .env_remove("CUOTAI_CONFIG")
        .env_remove("CUOTAI_USER_DIR")
        .env_remove("SEEDED_KEY")
        .env("CUOTAI_LANG", "en-US")
        .stdin(Stdio::null());
    for (key, value) in vars {
        command.env(key, value);
    }
    command.output().unwrap()
}

const CONFIG: &str = "# accounts\n[[accounts]]\nid = \"groq-work\"\nname = \"Private Name\"\nprovider = \"groq\"\ncredential_env = \"SEEDED_KEY\"\n";

#[test]
fn a_backup_restores_on_another_home_and_says_what_is_still_needed() {
    let first = home("first");
    fs::write(first.join("cuotai.toml"), CONFIG).unwrap();
    fs::create_dir_all(first.join("cuotai-data/serve")).unwrap();
    fs::write(first.join("cuotai-data/serve/token"), "SEEDED-SERVER-TOKEN").unwrap();
    let archive = first.join("settings.cuotai-backup.zip");
    let made = run(&first, &[], &["--backup", archive.to_str().unwrap()]);
    assert_eq!(
        made.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let bytes = fs::read(&archive).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("SEEDED-SERVER-TOKEN"));

    let second = home("second");
    let check = run(
        &second,
        &[],
        &["--restore", archive.to_str().unwrap(), "--check"],
    );
    assert_eq!(check.status.code(), Some(0));
    let plan = String::from_utf8_lossy(&check.stdout);
    assert!(plan.contains("config/cuotai.toml"), "{plan}");
    assert!(
        plan.contains("Set SEEDED_KEY for account 1 (groq)."),
        "{plan}"
    );
    assert!(
        !second.join("cuotai.toml").exists(),
        "--check writes nothing"
    );

    let applied = run(&second, &[], &["--restore", archive.to_str().unwrap()]);
    assert_eq!(
        applied.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    assert_eq!(
        fs::read_to_string(second.join("cuotai.toml")).unwrap(),
        CONFIG
    );

    // A conflicting local edit needs a terminal or --yes.
    fs::write(second.join("cuotai.toml"), "accounts = []\n").unwrap();
    let refused = run(&second, &[], &["--restore", archive.to_str().unwrap()]);
    assert_eq!(refused.status.code(), Some(1));
    assert_eq!(
        fs::read_to_string(second.join("cuotai.toml")).unwrap(),
        "accounts = []\n"
    );
    let forced = run(
        &second,
        &[],
        &["--restore", archive.to_str().unwrap(), "--yes"],
    );
    assert_eq!(forced.status.code(), Some(0));
    assert_eq!(
        fs::read_to_string(second.join("cuotai.toml")).unwrap(),
        CONFIG
    );

    // A damaged archive is exit 2.
    let mut broken = bytes.clone();
    let middle = broken.len() / 2;
    broken[middle] ^= 0x55;
    let damaged = second.join("damaged.cuotai-backup.zip");
    fs::write(&damaged, broken).unwrap();
    let out = run(
        &second,
        &[],
        &["--restore", damaged.to_str().unwrap(), "--check"],
    );
    assert_eq!(out.status.code(), Some(2));

    // The development credential store is never backed up.
    let refused = run(
        &first,
        &[],
        &[
            "--backup",
            first.join("dev.zip").to_str().unwrap(),
            "--dev-credentials-sqlite",
            first.join("dev.sqlite3").to_str().unwrap(),
        ],
    );
    assert_eq!(refused.status.code(), Some(1));
    assert!(!first.join("dev.zip").exists());
    let _ = fs::remove_dir_all(first);
    let _ = fs::remove_dir_all(second);
}

/// Where `dirs::config_dir()` points for a sandboxed `HOME`.
fn os_config_dirs(home: &Path) -> [PathBuf; 2] {
    [
        home.join("Library/Application Support/cuotai"),
        home.join("config/cuotai"),
    ]
}

#[test]
fn an_explicit_config_never_touches_this_computers_desktop_settings_or_folders() {
    let first = home("isolated");
    fs::write(first.join("cuotai.toml"), CONFIG).unwrap();
    for dir in os_config_dirs(&first) {
        fs::create_dir_all(dir.join("themes")).unwrap();
        fs::write(dir.join("desktop.json"), r#"{"motion":"off"}"#).unwrap();
        fs::write(dir.join("themes/night.css"), ":root{}").unwrap();
    }
    let run_isolated = |args: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cuotai"));
        command
            .args(["--config", first.join("cuotai.toml").to_str().unwrap()])
            .args(args)
            .env("HOME", &first)
            .env("XDG_DATA_HOME", first.join("data"))
            .env("XDG_CONFIG_HOME", first.join("config"))
            .env_remove("CUOTAI_CONFIG")
            .env_remove("CUOTAI_USER_DIR")
            .env_remove("CUOTAI_DATA_DIR")
            .env("CUOTAI_LANG", "en-US")
            .stdin(Stdio::null());
        command.output().unwrap()
    };
    let archive = first.join("isolated.cuotai-backup.zip");
    let made = run_isolated(&[
        "--backup",
        archive.to_str().unwrap(),
        "--with-history",
        "--json",
    ]);
    assert_eq!(
        made.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let manifest: serde_json::Value = serde_json::from_slice(&made.stdout).unwrap();
    let paths: Vec<_> = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["path"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(paths, ["config/cuotai.toml"], "only the named file");
    assert!(
        !first.join("data/cuotai").exists(),
        "no data folder created"
    );

    // A backup with desktop settings and a theme restored over the
    // isolated configuration leaves this computer's files alone.
    let source = home("isolated-source");
    fs::write(source.join("cuotai.toml"), CONFIG).unwrap();
    for dir in os_config_dirs(&source) {
        fs::create_dir_all(dir.join("themes")).unwrap();
        fs::write(dir.join("desktop.json"), r#"{"motion":"system"}"#).unwrap();
        fs::write(dir.join("themes/day.css"), ":root{}").unwrap();
    }
    let full = source.join("full.cuotai-backup.zip");
    let mut command = Command::new(env!("CARGO_BIN_EXE_cuotai"));
    let made = command
        .args(["--backup", full.to_str().unwrap()])
        .env("HOME", &source)
        .env("XDG_DATA_HOME", source.join("data"))
        .env("XDG_CONFIG_HOME", source.join("config"))
        .env("CUOTAI_DATA_DIR", source.join("cuotai-data"))
        .env_remove("CUOTAI_CONFIG")
        .env_remove("CUOTAI_USER_DIR")
        .current_dir(&source)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(
        made.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let restored = run_isolated(&["--restore", full.to_str().unwrap(), "--yes"]);
    assert_eq!(
        restored.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&restored.stderr)
    );
    let text = String::from_utf8_lossy(&restored.stdout);
    assert!(text.contains("Left out"), "{text}");
    assert!(text.contains("prefs/desktop.json"), "{text}");
    for dir in os_config_dirs(&first) {
        assert_eq!(
            fs::read_to_string(dir.join("desktop.json")).unwrap(),
            r#"{"motion":"off"}"#
        );
        assert!(!dir.join("themes/day.css").exists());
    }
    let _ = fs::remove_dir_all(first);
    let _ = fs::remove_dir_all(source);
}
