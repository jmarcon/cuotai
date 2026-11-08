//! PRD 0051 RF1/RF2: `--check-config` exits 0/1 with redacted human and JSON
//! diagnostics, and a runtime mode keeps the healthy accounts of a file that
//! also names a provider from a newer release.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cuotai-check-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("home")).unwrap();
    fs::create_dir_all(dir.join("bin")).unwrap();
    dir
}

fn run(dir: &Path, config: &str, args: &[&str]) -> Output {
    run_in(dir, config, args, "en-US")
}

fn run_in(dir: &Path, config: &str, args: &[&str], lang: &str) -> Output {
    let home = dir.join("home");
    let path = format!(
        "{}{}{}",
        dir.join("bin").display(),
        if cfg!(windows) { ";" } else { ":" },
        std::env::var("PATH").unwrap_or_default()
    );
    fs::write(dir.join("cuotai.toml"), config).unwrap();
    Command::new(env!("CARGO_BIN_EXE_cuotai"))
        .args(["--config", dir.join("cuotai.toml").to_str().unwrap()])
        .args(args)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("PATH", path)
        .env_remove("CUOTAI_CONFIG")
        .env_remove("CUOTAI_STATUS")
        .env("CUOTAI_LANG", lang)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

const VALID: &str =
    "[[accounts]]\nname = \"G\"\nprovider = \"groq\"\ncredential_env = \"GROQ_API_KEY\"\n";

#[test]
fn check_config_exits_0_for_a_valid_file_and_1_with_a_redacted_diagnostic() {
    let dir = sandbox("exit");
    let ok = run(&dir, VALID, &["--check-config"]);
    assert_eq!(ok.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&ok.stdout).contains("is valid"));

    for (text, path) in [
        ("accounts = [\n", serde_json::Value::Null),
        (
            "[[accounts]]\nprovider = \"groq\"\ncredential_env = \"K\"\nsecretish = \"sk-live-777\"\n",
            serde_json::json!("accounts[0].secretish"),
        ),
        (
            "[[accounts]]\nprovider = \"sk-live-777\"\n",
            serde_json::json!("accounts[0].provider"),
        ),
    ] {
        let human = run(&dir, text, &["--check-config"]);
        assert_eq!(human.status.code(), Some(1), "{text}");
        let shown = String::from_utf8_lossy(&human.stdout);
        assert!(shown.contains("line "), "{shown}");
        assert!(!shown.contains("sk-live"), "{shown}");
        let json = run(&dir, text, &["--check-config", "--json"]);
        assert_eq!(json.status.code(), Some(1));
        let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
        assert_eq!(value["valid"], false);
        assert_eq!(value["diagnostics"][0]["kind"], "parse");
        assert_eq!(value["diagnostics"][0]["path"], path);
        assert!(!String::from_utf8_lossy(&json.stdout).contains("sk-live"));
    }

    let semantic = run(
        &dir,
        "refresh_seconds = 1\naccounts = []\n",
        &["--check-config", "--json"],
    );
    assert_eq!(semantic.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&semantic.stdout).unwrap();
    assert_eq!(value["diagnostics"][0]["kind"], "bounds");

    let conflict = run(&dir, VALID, &["--check-config", "--once"]);
    assert_eq!(
        conflict.status.code(),
        Some(1),
        "an invalid combination is a local error"
    );
    let _ = fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn a_provider_from_a_newer_release_fails_only_its_account() {
    use std::os::unix::fs::PermissionsExt;
    let dir = sandbox("tolerant");
    let bl = dir.join("bin/bl");
    fs::write(
        &bl,
        "#!/bin/sh\nprintf '{\"per5HourPercentage\":0.4,\"per5HourResetTime\":%s000}\\n' $(( $(date +%s) + 10800 ))\n",
    )
    .unwrap();
    fs::set_permissions(&bl, fs::Permissions::from_mode(0o755)).unwrap();
    let config = "[[accounts]]\nname = \"Next\"\nprovider = \"a_provider_from_2030\"\n\n[[accounts]]\nname = \"Qwen\"\nprovider = \"alibaba_token_plan\"\nparams = { cli_profile = \"default\" }\n";
    let out = run(&dir, config, &["--json"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let snapshot: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let reports = snapshot["reports"].as_array().unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["account"], "Qwen");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("accounts[0].provider"), "{stderr}");
    assert!(!stderr.contains("a_provider_from_2030"), "{stderr}");
    let strict = run(&dir, config, &["--check-config"]);
    assert_eq!(strict.status.code(), Some(1));
    let _ = fs::remove_dir_all(dir);
}

/// PRD 0001 re-validation (M1): a configuration error found at startup is
/// printed in the chosen language, like `--check-config` prints it, by every
/// command that loads the file (a snapshot, the stored views, the watch).
#[test]
fn a_startup_configuration_error_is_printed_in_the_chosen_language() {
    let dir = sandbox("startup-l10n");
    let metered = "[[accounts]]\nname = \"Bedrock\"\nprovider = \"aws_bedrock\"\nrefresh_seconds = 120\nparams = { profile = \"fixture-profile\" }\n";
    for args in [
        &["--once"][..],
        &["--json"][..],
        &["--history", "Bedrock"][..],
        &["--timeline"][..],
        &["--history-export", "never-written.csv"][..],
    ] {
        let out = run_in(&dir, metered, args, "pt-BR");
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        let shown = String::from_utf8_lossy(&out.stderr);
        assert!(
            shown.contains("deve ser >= 300 para provedores cobrados por consulta"),
            "{args:?}: {shown}"
        );
        assert!(!shown.contains("must be >="), "{args:?}: {shown}");
    }
}
