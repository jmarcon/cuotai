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

/// How the fake server answers a `/health` challenge (re-validation S-03).
#[derive(Clone, Copy)]
enum Proof {
    /// Like `cuotai --serve` with this token: a valid proof.
    Valid,
    /// Like an older server or another program: `401` to the challenge.
    Missing,
}

/// A fake local server: `/health` with a challenge answers per `proof`,
/// `/v1/limits` answers `body` when the request carries `token` (`401`
/// otherwise). Returns its address and every request head it received
/// (request line first), read after the client process exits.
fn fake_server(
    token: &'static str,
    body: &'static str,
    proof: Proof,
) -> (String, std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let addr = format!("http://{}", listener.local_addr().unwrap());
    let heads = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = std::sync::Arc::clone(&heads);
    let secret = cuotai_core::serve::Token::parse(token).unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = Vec::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                    break;
                }
                head.push(line.trim().to_owned());
            }
            let header = |name: &str| {
                head.iter().find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case(name)
                        .then(|| value.trim().to_owned())
                })
            };
            let authorized = header("authorization").as_deref() == Some(&format!("Bearer {token}"));
            let (status, answer) = match (
                head.first().map(String::as_str),
                header("cuotai-challenge"),
            ) {
                (Some(line), Some(nonce)) if line.starts_with("GET /health ") => match proof {
                    Proof::Valid => (
                        "200 OK",
                        format!(
                            r#"{{"status":"ok","version":"0","schema":1,"schemas":[1],"auth":"token","proof":"{}"}}"#,
                            secret.proof(&nonce, port)
                        ),
                    ),
                    Proof::Missing => ("401 Unauthorized", String::new()),
                },
                (Some(line), _) if line.starts_with("GET /v1/limits ") && authorized => {
                    ("200 OK", body.to_owned())
                }
                _ => ("401 Unauthorized", String::new()),
            };
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            )
            .unwrap();
            seen.lock().unwrap().push(head);
        }
    });
    (addr, heads)
}

/// The request lines `heads` received, in order.
fn request_lines(heads: &std::sync::Mutex<Vec<Vec<String>>>) -> Vec<String> {
    heads
        .lock()
        .unwrap()
        .iter()
        .map(|head| head[0].clone())
        .collect()
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
    let (addr, heads) = fake_server(TOKEN, LIMITS, Proof::Valid);
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
        // Automatic names the provider (PRD 0025 re-validation V-07).
        "22% · Claude"
    );
    // S-03: the challenge first, then the token.
    assert_eq!(
        request_lines(&heads),
        ["GET /health HTTP/1.1", "GET /v1/limits HTTP/1.1"]
    );
    let heads = heads.lock().unwrap();
    assert!(!heads[0]
        .iter()
        .any(|line| line.to_ascii_lowercase().starts_with("authorization")));
}

/// S-03: a program on the port that cannot prove it holds the token (an
/// older server, or anything else) never receives it, and the line is a
/// local error that says to update the server.
#[test]
fn from_server_never_sends_the_token_to_an_unproven_program() {
    let sandbox = Sandbox::new("from-server-unproven");
    let token = sandbox.0.join("token");
    fs::write(&token, format!("{TOKEN}\n")).unwrap();
    let (addr, heads) = fake_server(TOKEN, LIMITS, Proof::Missing);
    let output = sandbox.run(
        "dev",
        &[
            "--from-server",
            &addr,
            "--token-file",
            token.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("did not prove") && stderr.contains("update Cuotai"),
        "{stderr}"
    );
    assert_eq!(request_lines(&heads), ["GET /health HTTP/1.1"]);
    let heads = heads.lock().unwrap();
    assert!(heads.iter().flatten().all(|line| !line.contains(TOKEN)));
}

#[test]
fn from_server_refuses_other_hosts_and_a_missing_token() {
    let sandbox = Sandbox::new("from-server-errors");
    let output = sandbox.run("dev", &["--from-server", "http://192.0.2.1:7878"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("loopback"));
    let (addr, _heads) = fake_server(TOKEN, LIMITS, Proof::Valid);
    let output = sandbox.run("dev", &["--from-server", &addr]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("token"));
}

/// S-04: text a server sends (here an OSC 0 title, an OSC 52 clipboard
/// write, CSI and a bidi override in the name) never reaches the terminal.
#[test]
fn from_server_strips_control_and_direction_characters() {
    const EVIL: &str = r#"{"schema":1,"generated_at":"2026-10-25T12:00:00Z","accounts":[{"index":0,"id":"acc_one","key":"claude:Main","name":"\u001b]0;pwned\u0007Ma\u202ein\u001b]52;c;aGk=\u0007\u001b[2J","provider":"claude","class":"available","stale":false,"binding":{"used_ratio":0.78}}],"summary":{}}"#;
    let sandbox = Sandbox::new("from-server-evil");
    let token = sandbox.0.join("token");
    fs::write(&token, format!("{TOKEN}\n")).unwrap();
    let (addr, _heads) = fake_server(TOKEN, EVIL, Proof::Valid);
    let output = sandbox.run(
        "dev",
        &[
            "--from-server",
            &addr,
            "--token-file",
            token.to_str().unwrap(),
            "--account",
            "acc_one",
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let line = String::from_utf8(output.stdout).unwrap();
    assert!(
        !line
            .trim_end()
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')),
        "{line:?}"
    );
    assert!(line.contains("22%"), "{line:?}");
}

/// Re-validation V-04: a selected account that failed (no preserved
/// reading, so not `stale`) exits 2 from a server too, like without one.
#[test]
fn from_server_exits_2_for_a_failed_account() {
    const FAILED: &str = r#"{"schema":1,"generated_at":"2026-10-25T12:00:00Z","accounts":[{"index":0,"id":"acc_one","key":"claude:Main","name":"Main","provider":"claude","class":"unmeasured","stale":false,"binding":null,"failure":{"category":"auth_required"}}],"summary":{}}"#;
    let sandbox = Sandbox::new("from-server-failed");
    let token = sandbox.0.join("token");
    fs::write(&token, format!("{TOKEN}\n")).unwrap();
    let (addr, _heads) = fake_server(TOKEN, FAILED, Proof::Valid);
    let output = sandbox.run(
        "dev",
        &[
            "--from-server",
            &addr,
            "--token-file",
            token.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
}
