//! PRD 0044 RF5.2/RF5.3: `--channel-add` with the address piped on
//! standard input and `--yes`, as the guide shows, in a sandboxed home with
//! the debug-only SQLite credential store (never the system vault). Adding
//! a channel sends nothing; the address points at a closed local port.

#![cfg(all(unix, debug_assertions))]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn home(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("cuotai-channels-cli-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(home: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cuotai"))
        .args(["--config", home.join("cuotai.toml").to_str().unwrap()])
        .args([
            "--dev-credentials-sqlite",
            home.join("dev.sqlite3").to_str().unwrap(),
        ])
        .args(args)
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("CUOTAI_DATA_DIR", home.join("cuotai-data"))
        .env("CUOTAI_USER_DIR", home.join("user"))
        .env("CUOTAI_CONFIG", home.join("cuotai.toml"))
        .env("CUOTAI_LANG", "en-US")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(stdin.as_bytes()).unwrap();
    drop(input);
    child.wait_with_output().unwrap()
}

/// Every `desktop.json` under `dir` (its folder depends on the OS).
fn settings_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            settings_files(&path, found);
        } else if path.file_name().is_some_and(|name| name == "desktop.json") {
            found.push(path);
        }
    }
}

#[test]
fn a_piped_address_is_saved_with_yes_and_never_written_out() {
    let home = home("add");
    fs::write(home.join("cuotai.toml"), "accounts = []\n").unwrap();
    let added = run(
        &home,
        &["--channel-add", "ntfy", "--label", "Phone", "--yes"],
        "http://127.0.0.1:9/t\n",
    );
    let (out, err) = (
        String::from_utf8_lossy(&added.stdout),
        String::from_utf8_lossy(&added.stderr),
    );
    assert_eq!(added.status.code(), Some(0), "{out}\n{err}");
    assert!(out.contains("saved and on"), "{out}");

    let listed = run(&home, &["--channel-list", "--json"], "");
    assert_eq!(listed.status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    let host = json["channels"][0]["host"].as_str().unwrap();
    assert!(host.starts_with("127.0.0.1"), "{host}");
    assert!(!host.contains("/t"), "{host}");

    let mut found = Vec::new();
    settings_files(&home, &mut found);
    assert_eq!(found.len(), 1, "{found:?}");
    let saved = fs::read_to_string(&found[0]).unwrap();
    assert!(!saved.contains("/t\""), "{saved}");
    assert!(!saved.contains("127.0.0.1:9/t"), "{saved}");
    for text in [&out, &err] {
        assert!(!text.contains("127.0.0.1:9/t"), "{text}");
    }
    let _ = fs::remove_dir_all(&home);
}

/// `--yes` stays refused where nothing reads it, `--channel-remove`
/// included.
#[test]
fn yes_is_refused_by_channel_remove() {
    let home = home("remove");
    fs::write(home.join("cuotai.toml"), "accounts = []\n").unwrap();
    let refused = run(&home, &["--channel-remove", "c1", "--yes"], "");
    assert_eq!(refused.status.code(), Some(1));
    let err = String::from_utf8_lossy(&refused.stderr);
    assert!(err.contains("--channel-add"), "{err}");
    let _ = fs::remove_dir_all(&home);
}
