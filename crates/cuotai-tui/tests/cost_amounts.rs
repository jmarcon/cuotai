//! PRD 0041 re-validation (F-05): `--cost-plan` and `--cost-currency` read
//! amounts like the budget editors and refuse one such as `1,000` that
//! reads two ways, instead of saving 1. Runs in a sandboxed home and data
//! folder.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cuotai-costamt-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("home")).unwrap();
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

fn settings(dir: &Path) -> String {
    fs::read_to_string(dir.join("data/cost/settings.json")).unwrap_or_default()
}

#[test]
fn an_ambiguous_plan_price_is_refused_and_a_clear_one_saved() {
    let dir = sandbox("plan");
    let refused = run(&dir, &["--cost-plan", "claude:Work", "1,000", "USD"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("PRICE: 1,000 could be 1000 or 1"),
        "{stderr}"
    );
    assert!(!settings(&dir).contains("plans"), "nothing saved");

    let saved = run(&dir, &["--cost-plan", "claude:Work", "1,000.00", "USD"]);
    assert!(saved.status.success(), "{saved:?}");
    assert!(settings(&dir).contains("1000"), "{}", settings(&dir));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_ambiguous_rate_is_refused() {
    let dir = sandbox("rate");
    let refused = run(&dir, &["--cost-currency", "BRL", "5,250"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("RATE: 5,250 could be 5250 or 5.25"),
        "{stderr}"
    );
    assert!(!settings(&dir).contains("BRL"));
    let saved = run(&dir, &["--cost-currency", "BRL", "5,25"]);
    assert!(saved.status.success(), "{saved:?}");
    assert!(settings(&dir).contains("5.25"), "{}", settings(&dir));
    let _ = fs::remove_dir_all(&dir);
}
