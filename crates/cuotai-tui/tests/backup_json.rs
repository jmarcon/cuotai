//! PRD 0046 re-validation R-01: `--restore FILE --json` prints one JSON
//! document on stdout; the done line goes to stderr. Sandboxed homes, no
//! network, no vault.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn home(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("cuotai-bundle-json-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cuotai"))
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
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

const CONFIG: &str = "[[accounts]]\nid = \"groq-work\"\nname = \"Work\"\nprovider = \"groq\"\ncredential_env = \"SEEDED_KEY\"\n";

#[test]
fn restore_json_prints_only_json_on_stdout() {
    let first = home("first");
    fs::write(first.join("cuotai.toml"), CONFIG).unwrap();
    let archive = first.join("settings.cuotai-backup.zip");
    let made = run(&first, &["--backup", archive.to_str().unwrap()]);
    assert_eq!(made.status.code(), Some(0));

    let second = home("second");
    fs::write(second.join("cuotai.toml"), "accounts = []\n").unwrap();
    let out = run(
        &second,
        &["--restore", archive.to_str().unwrap(), "--json", "--yes"],
    );
    assert_eq!(out.status.code(), Some(0));
    let plan: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(plan["items"].is_array());
    assert!(String::from_utf8_lossy(&out.stderr).contains("Files restored: 1."));
    assert_eq!(
        fs::read_to_string(second.join("cuotai.toml")).unwrap(),
        CONFIG
    );
    let _ = fs::remove_dir_all(first);
    let _ = fs::remove_dir_all(second);
}
