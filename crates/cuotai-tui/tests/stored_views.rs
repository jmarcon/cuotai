//! PRD 0026 RF6.2: `--stats`, `--timeline` and `--events-log` read the
//! history store and the last recorded readings without querying anything.
//! One `--once` against a fake `bl` (Alibaba Token Plan CLI) records a
//! reading; every flag combination is then judged by its output and exit
//! code.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cuotai-stored-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::create_dir_all(dir.join("home")).unwrap();
        let bl = dir.join("bin/bl");
        fs::write(
            &bl,
            r#"#!/bin/sh
printf '{"per5HourPercentage":0.4,"per5HourResetTime":%s000}\n' $(( $(date +%s) + 10800 ))
"#,
        )
        .unwrap();
        fs::set_permissions(&bl, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(
            dir.join("cuotai.toml"),
            "[[accounts]]\nname = \"Qwen\"\nprovider = \"alibaba_token_plan\"\nparams = { cli_profile = \"default\" }\n",
        )
        .unwrap();
        Self(dir)
    }

    fn run(&self, args: &[&str]) -> Output {
        let home = self.0.join("home");
        let path = format!(
            "{}:{}",
            self.0.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new(env!("CARGO_BIN_EXE_cuotai"))
            .args(["--config", self.0.join("cuotai.toml").to_str().unwrap()])
            .args(args)
            .env("HOME", &home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("PATH", path)
            .env_remove("CUOTAI_CONFIG")
            .env_remove("CUOTAI_STATUS")
            .env("CUOTAI_LANG", "pt-BR")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// Both places the OS may keep the history store (Linux honors
    /// `XDG_DATA_HOME`, macOS uses `~/Library`).
    fn history_files(&self) -> [PathBuf; 2] {
        let home = self.0.join("home");
        [
            home.join("data/cuotai/history/history.sqlite3"),
            home.join("Library/Application Support/cuotai/history/history.sqlite3"),
        ]
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap()
}

#[test]
fn nothing_recorded_yet_is_not_enough_history_and_creates_no_store() {
    let sandbox = Sandbox::new("empty");
    let stats = sandbox.run(&["--stats"]);
    assert_eq!(code(&stats), 0);
    let out = text(&stats);
    assert!(out.contains("Histórico insuficiente"), "{out}");
    assert!(
        out.contains("Pico médio do limite mais apertado: —"),
        "{out}"
    );
    let timeline = sandbox.run(&["--timeline"]);
    assert_eq!(code(&timeline), 0);
    assert!(text(&timeline).contains("Nenhum retorno neste período."));
    let events = sandbox.run(&["--events-log"]);
    assert_eq!(code(&events), 0);
    assert!(text(&events).contains("Nenhum evento registrado."));
    assert!(sandbox.history_files().iter().all(|path| !path.exists()));
}

#[test]
fn flags_read_what_a_once_recorded() {
    let sandbox = Sandbox::new("recorded");
    let once = sandbox.run(&["--once"]);
    assert_eq!(
        code(&once),
        0,
        "{}{}",
        text(&once),
        String::from_utf8_lossy(&once.stderr)
    );

    let stats = sandbox.run(&["--stats", "--days", "30", "--json"]);
    assert_eq!(code(&stats), 0);
    let json: serde_json::Value = serde_json::from_slice(&stats.stdout).unwrap();
    assert_eq!(json["range"], "days30");
    assert_eq!(json["daily"].as_array().unwrap().len(), 30);
    assert_eq!(json["accounts"][0]["name"], "Qwen");
    assert_eq!(json["coverage"]["kind"], "not_enough", "one day of data");

    let timeline = sandbox.run(&["--timeline"]);
    assert_eq!(code(&timeline), 0);
    let out = text(&timeline);
    assert!(out.contains("próximas 24 h"), "{out}");
    assert!(out.contains("Qwen") && out.contains("restante"), "{out}");
    let json = sandbox.run(&["--timeline", "--range", "7d", "--json"]);
    assert_eq!(code(&json), 0);
    let json: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(json["range"], "days7");
    assert_eq!(json["lanes"][0]["name"], "Qwen");

    let events = sandbox.run(&["--events-log", "--days", "30", "--json"]);
    assert_eq!(code(&events), 0);
    let json: serde_json::Value = serde_json::from_slice(&events.stdout).unwrap();
    assert_eq!(json["events"].as_array().unwrap().len(), 0);
}

#[test]
fn invalid_arguments_are_local_errors() {
    let sandbox = Sandbox::new("invalid");
    for args in [
        &["--stats", "--days", "14"][..],
        &["--events-log", "--days", "0"],
        &["--events-log", "--days", "366"],
        &["--timeline", "--range", "48h"],
        &["--range", "7d"],
        &["--days", "7"],
        &["--stats", "--timeline"],
        &["--stats", "--once"],
        &["--timeline", "--watch"],
        &["--events-log", "--history"],
    ] {
        let output = sandbox.run(args);
        assert_eq!(code(&output), 1, "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
    }
}

#[test]
fn an_unreadable_store_exits_two() {
    let sandbox = Sandbox::new("broken");
    for path in sandbox.history_files() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, vec![0x7f; 4096]).unwrap();
    }
    for flag in ["--stats", "--timeline", "--events-log"] {
        let output = sandbox.run(&[flag]);
        assert_eq!(code(&output), 2, "{flag}");
        assert!(output.stdout.is_empty(), "{flag}");
    }
}
