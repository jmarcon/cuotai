//! PRD 0042 re-validation (K7): `--top` is refused where it does not apply
//! and as 0 (L-05, exit 1), and `--cost-live --source` with that source
//! off names it and exits 2 (L-06). Everything runs in a sandboxed home
//! and data folder; no log is read.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cuotai-costby-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("home")).unwrap();
    fs::create_dir_all(dir.join("data/cost")).unwrap();
    fs::write(dir.join("cuotai.toml"), "").unwrap();
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
fn top_is_refused_where_it_does_not_apply() {
    let dir = sandbox("top");
    for args in [
        &["--cost", "--by", "session", "--top", "0"][..],
        &["--cost", "--by", "block", "--top", "5"],
        &["--cost", "--by", "day", "--all"],
    ] {
        let out = run(&dir, args);
        assert_eq!(out.status.code(), Some(1), "{args:?}: {out:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("apply only to --cost --by session or --by project"),
            "{args:?}: {stderr}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_live_view_of_a_source_that_is_off_exits_2_and_names_it() {
    let dir = sandbox("live");
    let out = run(&dir, &["--cost-live", "--source", "codex_cli"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Codex CLI is off; cuotai --cost-enable codex_cli turns it on."),
        "{stdout}"
    );
    let json = run(&dir, &["--cost-live", "--source", "gemini_cli", "--json"]);
    assert_eq!(json.status.code(), Some(2), "{json:?}");
    assert!(json.stdout.is_empty(), "{json:?}");
    let _ = fs::remove_dir_all(&dir);
}
