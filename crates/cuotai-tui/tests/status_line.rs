//! PRD 0031 RF2: `cuotai --status-line` prints one plain line from the
//! core's tray summary after one refresh, with a fake AWS CLI as the only
//! provider, and exits like `--once`.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("cuotai-statusline-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::create_dir_all(dir.join("home")).unwrap();
        let aws = dir.join("bin/aws");
        fs::write(
            &aws,
            r#"#!/bin/sh
case "$*" in
  *"--profile broken"*) echo 'boom' >&2; exit 254;;
  *) echo '{"ResultsByTime":[{"Total":{"UnblendedCost":{"Amount":"12.34","Unit":"USD"}},"Estimated":false}]}';;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(&aws, fs::Permissions::from_mode(0o755)).unwrap();
        Self(dir)
    }

    fn run(&self, profile: &str, extra: &[&str]) -> Output {
        let config = self.0.join(format!("{profile}.toml"));
        fs::write(
            &config,
            format!(
                "[[accounts]]\nname = \"Bedrock\"\nprovider = \"aws_bedrock\"\nparams = {{ profile = \"{profile}\" }}\n"
            ),
        )
        .unwrap();
        let home = self.0.join("home");
        let path = format!(
            "{}:{}",
            self.0.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new(env!("CARGO_BIN_EXE_cuotai"))
            .args(["--status-line", "--config"])
            .arg(&config)
            .args(extra)
            .env("HOME", &home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("PATH", path)
            .env("CUOTAI_LANG", "en-US")
            .env_remove("CUOTAI_CONFIG")
            .env_remove("NO_COLOR")
            .stdin(Stdio::null())
            .output()
            .expect("run cuotai")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A ready account without a plan limit has no share: the honest "—" (the
/// line never invents 0 %), one line, no ANSI, exit 0.
#[test]
fn prints_one_plain_line_and_exits_0_for_a_ready_account() {
    let sandbox = Sandbox::new("ready");
    let output = sandbox.run("dev", &[]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), 1, "{text:?}");
    assert!(!text.contains('\x1b'), "{text:?}");
    assert_eq!(text.trim(), "—");
}

#[test]
fn a_failed_account_exits_2_and_color_needs_a_terminal() {
    let sandbox = Sandbox::new("failed");
    let output = sandbox.run("broken", &["--color", "--style", "logos_percent"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    // Stdout is a pipe here; NO_COLOR is covered by the unit tests.
    assert!(!String::from_utf8(output.stdout).unwrap().contains('\x1b'));
}

#[test]
fn style_must_be_supported_and_modes_do_not_combine() {
    let sandbox = Sandbox::new("args");
    assert_eq!(sandbox.run("dev", &["--style", "meters"]).status.code(), Some(1));
    assert_eq!(sandbox.run("dev", &["--once"]).status.code(), Some(1));
}
