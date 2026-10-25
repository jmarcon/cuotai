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
    assert_eq!(
        sandbox.run("dev", &["--style", "meters"]).status.code(),
        Some(1)
    );
    assert_eq!(sandbox.run("dev", &["--once"]).status.code(), Some(1));
}

/// A one-shot HTTP server that answers `/v1/limits` with `body` when the
/// request carries `token`, `401` otherwise, and returns the request line.
fn fake_server(
    token: &'static str,
    body: &'static str,
) -> (String, std::thread::JoinHandle<String>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", listener.local_addr().unwrap());
    let job = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut head = Vec::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line.trim().is_empty() {
                break;
            }
            head.push(line.trim().to_owned());
        }
        let authorized = head
            .iter()
            .any(|line| line.eq_ignore_ascii_case(&format!("authorization: Bearer {token}")));
        let mut stream = stream;
        if authorized {
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        } else {
            write!(
                stream,
                "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        }
        head[0].clone()
    });
    (addr, job)
}

const LIMITS: &str = r#"{"schema":1,"generated_at":"2026-10-25T12:00:00Z","accounts":[{"index":0,"id":"acc_one","key":"claude:Main","name":"Main","provider":"claude","class":"available","stale":false,"binding":{"used_ratio":0.78}}],"summary":{}}"#;
const TOKEN: &str = "cuotai_test-token-0123456789abcdef";

/// PRD 0031 RF2.1: `--from-server` reads `/v1/limits` with the token file
/// instead of querying (the fake `aws` is never needed) and exits 0.
#[test]
fn from_server_reads_the_running_servers_limits() {
    let sandbox = Sandbox::new("from-server");
    let token = sandbox.0.join("token");
    fs::write(&token, format!("{TOKEN}\n")).unwrap();
    let (addr, job) = fake_server(TOKEN, LIMITS);
    let output = sandbox.run(
        "broken",
        &[
            "--from-server",
            &addr,
            "--token-file",
            token.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "22% · Main"
    );
    assert_eq!(job.join().unwrap(), "GET /v1/limits HTTP/1.1");
}

#[test]
fn from_server_refuses_other_hosts_and_a_missing_token() {
    let sandbox = Sandbox::new("from-server-errors");
    let output = sandbox.run("dev", &["--from-server", "http://192.0.2.1:7878"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("loopback"));
    let (addr, job) = fake_server(TOKEN, LIMITS);
    let output = sandbox.run("dev", &["--from-server", &addr]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("token"));
    job.join().unwrap();
}
