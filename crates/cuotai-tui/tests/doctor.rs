//! PRD 0046 RF4: `--doctor` from the command line, in a sandboxed home,
//! with no network and no vault.

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
fn the_doctor_names_a_missing_variable_without_any_value() {
    let dir = home("doctor");
    fs::write(dir.join("cuotai.toml"), CONFIG).unwrap();
    let missing = run(&dir, &[], &["--doctor"]);
    assert_eq!(missing.status.code(), Some(2));
    let text = String::from_utf8_lossy(&missing.stdout);
    assert!(text.contains("SEEDED_KEY"), "{text}");
    assert!(!text.contains("Private Name"), "{text}");
    assert!(text.contains("~"), "home paths are abbreviated: {text}");

    let set = run(
        &dir,
        &[("SEEDED_KEY", "sk-live-0123456789abcdefSEEDED")],
        &["--doctor", "--json"],
    );
    assert_eq!(
        set.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&set.stdout)
    );
    let json = String::from_utf8_lossy(&set.stdout);
    assert!(!json.contains("SEEDED\""), "{json}");
    assert!(!json.contains("sk-live"), "{json}");
    let report: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(report["schema"], 1);
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["id"] == "account.1" && c["status"] == "ok"));
    let _ = fs::remove_dir_all(dir);
}
