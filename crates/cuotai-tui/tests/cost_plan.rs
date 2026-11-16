//! PRD 0030 L-06 and C-02: `--cost-plan` saves only under a configured
//! account, and `--cost` never prints a value from a hand-edited settings
//! file. Everything runs in a sandboxed home and data folder.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cuotai-costplan-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("home")).unwrap();
    fs::create_dir_all(dir.join("data/cost")).unwrap();
    fs::write(
        dir.join("cuotai.toml"),
        "[[accounts]]\nid = \"acct-work\"\nname = \"Work\"\nprovider = \"claude\"\n",
    )
    .unwrap();
    dir
}

fn run(dir: &Path, args: &[&str]) -> Output {
    let home = dir.join("home");
    Command::new(env!("CARGO_BIN_EXE_cuotai"))
        .args(["--config", dir.join("cuotai.toml").to_str().unwrap()])
        .args(args)
        .env("HOME", &home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("CUOTAI_DATA_DIR", dir.join("data"))
        .env("CUOTAI_USER_DIR", dir.join("user"))
        .env_remove("CUOTAI_CONFIG")
        .env("CUOTAI_LANG", "en-US")
        .output()
        .expect("run cuotai")
}

#[test]
fn a_plan_needs_a_configured_account() {
    let dir = sandbox("plan");
    let settings = dir.join("data/cost/settings.json");
    let refused = run(&dir, &["--cost-plan", "claude:nobody", "20", "USD"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("No configured account is named claude:nobody"),
        "{stderr}"
    );
    assert!(!fs::read_to_string(&settings)
        .unwrap_or_default()
        .contains("plans"));

    let saved = run(&dir, &["--cost-plan", "claude:Work", "20", "USD"]);
    assert!(saved.status.success(), "{saved:?}");
    let raw = fs::read_to_string(&settings).unwrap();
    assert!(raw.contains("acct-work"), "saved under the id: {raw}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn hostile_settings_never_reach_the_cost_text() {
    let dir = sandbox("hostile");
    fs::write(
        dir.join("data/cost/settings.json"),
        r#"{"plans":{"acct-work":{"price":-20,"currency":"\u001b[31mEUR"}},"display_currency":{"code":"\u001b]0;X\u0007","rate":-3,"set_on":"2026-10-01"}}"#,
    )
    .unwrap();
    let out = run(&dir, &["--cost"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains('\u{1b}') && !stdout.contains("EUR"),
        "{stdout}"
    );
    assert!(stdout.contains("1 saved plan price is invalid"), "{stdout}");
    assert!(stdout.contains("second currency is invalid"), "{stdout}");
    let _ = fs::remove_dir_all(&dir);
}
