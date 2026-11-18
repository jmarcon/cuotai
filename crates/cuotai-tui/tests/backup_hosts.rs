//! PRD 0046 M-03 (owner decision of 2026-11-18): `--restore` lists each
//! account that would send a key to another address, and writes only with
//! `--accept-hosts` (or a `y` in a terminal); `--yes` alone is not enough.
//! Sandboxed homes, no network, no vault.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cuotai-hosts-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// `cuotai` with the piped (non-terminal) standard input.
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
        .env("CUOTAI_LANG", "en-US")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

const CONFIG: &str = "[[accounts]]\nid = \"proxy\"\nprovider = \"litellm\"\ncredential_env = \"PROXY_KEY\"\n\n[accounts.params]\nbase_url = \"https://proxy.example.com:8443/v1\"\n";

#[test]
fn a_restore_sending_keys_elsewhere_needs_accept_hosts() {
    let first = home("first");
    fs::write(first.join("cuotai.toml"), CONFIG).unwrap();
    let archive = first.join("b.cuotai-backup.zip");
    let made = run(&first, &["--backup", archive.to_str().unwrap()]);
    assert_eq!(made.status.code(), Some(0));
    let archive = archive.to_str().unwrap();

    let second = home("second");
    let check = run(&second, &["--restore", archive, "--check"]);
    assert_eq!(check.status.code(), Some(0));
    let plan = String::from_utf8_lossy(&check.stdout);
    assert!(
        plan.contains("  ! Account 1 (litellm) sends its key to https://proxy.example.com:8443."),
        "the plan names the host"
    );
    assert!(!plan.contains("/v1"), "never the path");

    let json = run(&second, &["--restore", archive, "--check", "--json"]);
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["hosts"][0]["host"], "https://proxy.example.com:8443");

    let refused = run(&second, &["--restore", archive, "--yes"]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--accept-hosts"));
    assert!(
        !second.join("cuotai.toml").exists(),
        "--yes alone writes nothing"
    );

    let accepted = run(&second, &["--restore", archive, "--yes", "--accept-hosts"]);
    assert_eq!(accepted.status.code(), Some(0));
    assert_eq!(
        fs::read_to_string(second.join("cuotai.toml")).unwrap(),
        CONFIG
    );
    let _ = fs::remove_dir_all(first);
    let _ = fs::remove_dir_all(second);
}
