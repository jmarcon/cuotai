//! PRD 0051 RF5.2/RF5.3: `CUOTAI_DATA_DIR` moves every local store, and an
//! invalid value is a local error rather than a fallback to the OS folder.
//! PRD 0051 re-validation S-01/S-02: an existing folder that is not Cuotai's
//! gets a `Cuotai` subfolder, and an older Cuotai folder is used as is;
//! neither is narrowed nor loses a file.

#![cfg(unix)]

mod common;

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
    common::write_executable(
        &bl,
        "#!/bin/sh\nprintf '{\"per5HourPercentage\":0.4,\"per5HourResetTime\":%s000}\\n' $(( $(date +%s) + 10800 ))\n",
    )
    .unwrap();
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

/// S-01/S-02: someone else's folder keeps its files and its mode; Cuotai's
/// stores go into `<folder>/Cuotai`, marked and private.
#[test]
fn an_existing_foreign_folder_gets_a_cuotai_subfolder_and_loses_nothing() {
    let dir = sandbox("foreign");
    let data = dir.join("MyProjects");
    fs::create_dir_all(data.join("feed")).unwrap();
    fs::write(data.join("feed").join("notes.txt"), b"mine").unwrap();
    fs::set_permissions(&data, fs::Permissions::from_mode(0o755)).unwrap();
    let out = run(&dir, data.to_str().unwrap(), &["--once"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read(data.join("feed").join("notes.txt")).unwrap(),
        b"mine"
    );
    assert_eq!(
        fs::metadata(&data).unwrap().permissions().mode() & 0o777,
        0o755
    );
    let used = data.join("Cuotai");
    assert!(used.join("history").join("history.sqlite3").is_file());
    let marker = used.join(".cuotai-data");
    assert!(marker.is_file());
    assert_eq!(
        fs::metadata(&marker).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&used).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(!data.join("logs").exists() && !data.join("history").exists());
    let _ = fs::remove_dir_all(dir);
}

/// S-02: a folder an older Cuotai used (only its log is there) is used as
/// is and never narrowed again.
#[test]
fn an_older_cuotai_folder_is_used_as_is() {
    let dir = sandbox("older");
    let data = dir.join("old-profile");
    fs::create_dir_all(data.join("logs")).unwrap();
    fs::write(data.join("logs").join("cuotai.log"), b"").unwrap();
    fs::set_permissions(&data, fs::Permissions::from_mode(0o755)).unwrap();
    let out = run(&dir, data.to_str().unwrap(), &["--once"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(data.join("history").join("history.sqlite3").is_file());
    assert!(!data.join("Cuotai").exists());
    assert_eq!(
        fs::metadata(&data).unwrap().permissions().mode() & 0o777,
        0o755
    );
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
