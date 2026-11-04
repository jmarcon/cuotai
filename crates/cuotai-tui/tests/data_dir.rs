//! PRD 0051 RF5.2/RF5.3: `CUOTAI_DATA_DIR` moves every local store, and an
//! invalid value is a local error rather than a fallback to the OS folder.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cuotai-datadir-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("bin")).unwrap();
    fs::create_dir_all(dir.join("home")).unwrap();
    let bl = dir.join("bin/bl");
    fs::write(
        &bl,
        "#!/bin/sh\nprintf '{\"per5HourPercentage\":0.4,\"per5HourResetTime\":%s000}\\n' $(( $(date +%s) + 10800 ))\n",
    )
    .unwrap();
    fs::set_permissions(&bl, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        dir.join("cuotai.toml"),
        "[[accounts]]\nname = \"Qwen\"\nprovider = \"alibaba_token_plan\"\nparams = { cli_profile = \"default\" }\n",
    )
    .unwrap();
    dir
}

fn run(dir: &Path, data: &str, args: &[&str]) -> Output {
    let home = dir.join("home");
    Command::new(env!("CARGO_BIN_EXE_cuotai"))
        .args(["--config", dir.join("cuotai.toml").to_str().unwrap()])
        .args(args)
        .env("HOME", &home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env(
            "PATH",
            format!(
                "{}:{}",
                dir.join("bin").display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("CUOTAI_DATA_DIR", data)
        .env_remove("CUOTAI_CONFIG")
        .env_remove("CUOTAI_STATUS")
        .env("CUOTAI_LANG", "en-US")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn the_history_lands_in_the_override_and_not_in_the_os_folder() {
    let dir = sandbox("moved");
    let data = dir.join("profile");
    let out = run(&dir, data.to_str().unwrap(), &["--once"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(data.join("history").join("history.sqlite3").is_file());
    assert_eq!(
        fs::metadata(&data).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let os_default = [
        dir.join("home/data/cuotai"),
        dir.join("home/Library/Application Support/cuotai"),
    ];
    for path in os_default {
        assert!(!path.exists(), "{} was touched", path.display());
    }
    let history = run(&dir, data.to_str().unwrap(), &["--history", "--json"]);
    assert_eq!(history.status.code(), Some(0));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn an_invalid_override_is_a_local_error() {
    let dir = sandbox("invalid");
    let out = run(&dir, "relative/data", &["--once"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("CUOTAI_DATA_DIR"));
    let file = dir.join("a-file");
    fs::write(&file, b"x").unwrap();
    let out = run(&dir, file.to_str().unwrap(), &["--once"]);
    assert_eq!(out.status.code(), Some(1));
    let _ = fs::remove_dir_all(dir);
}
