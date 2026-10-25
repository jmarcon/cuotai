//! PRD 0012 RF4: `cuotai --serve` as a script runs it: the binary in the
//! background with a fake AWS CLI as the only provider, read over HTTP,
//! stopped with a signal, judged by its exit code.

#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const TOKEN: &str = "cuotai_test-token-0123456789abcdef";

/// A private HOME with a fake `aws` that logs every call and answers by
/// profile: `dev` with a Cost Explorer total, `broken` with a failure.
struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cuotai-serve-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::create_dir_all(dir.join("home")).unwrap();
        let aws = dir.join("bin/aws");
        fs::write(
            &aws,
            format!(
                r#"#!/bin/sh
echo "$@" >> '{log}'
case "$*" in
  *"--profile broken"*) echo 'boom' >&2; exit 254;;
  *) echo '{{"ResultsByTime":[{{"Total":{{"UnblendedCost":{{"Amount":"12.34","Unit":"USD"}}}},"Estimated":false}}]}}';;
esac
"#,
                log = dir.join("aws-calls.log").display()
            ),
        )
        .unwrap();
        fs::set_permissions(&aws, fs::Permissions::from_mode(0o755)).unwrap();
        Self(dir)
    }

    fn config(&self, profile: &str) -> PathBuf {
        let path = self.0.join(format!("{profile}.toml"));
        fs::write(
            &path,
            format!(
                "[[accounts]]\nname = \"Bedrock\"\nprovider = \"aws_bedrock\"\nparams = {{ profile = \"{profile}\" }}\n"
            ),
        )
        .unwrap();
        path
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }

    fn aws_calls(&self) -> usize {
        fs::read_to_string(self.0.join("aws-calls.log")).map_or(0, |log| log.lines().count())
    }

    fn command(&self, args: &[&str]) -> Command {
        let home = self.0.join("home");
        let path = format!(
            "{}:{}",
            self.0.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut command = Command::new(env!("CARGO_BIN_EXE_cuotai"));
        command
            .args(args)
            .env("HOME", &home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("PATH", path)
            .env("CUOTAI_LANG", "en-US")
            .env_remove("CUOTAI_CONFIG")
            .env_remove("CUOTAI_STATUS")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(
                fs::File::create(self.0.join("stderr.log")).unwrap(),
            ));
        command
    }

    fn stderr(&self) -> String {
        fs::read_to_string(self.0.join("stderr.log")).unwrap_or_default()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A running `--serve` and the address it printed on stdout.
struct Server {
    child: Child,
    addr: String,
}

impl Server {
    fn start(sandbox: &Sandbox, args: &[&str]) -> Self {
        let mut child = sandbox.command(args).spawn().unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let addr = line
            .trim()
            .strip_prefix("http://")
            .unwrap_or_else(|| panic!("no address on stdout: {line:?}; {}", sandbox.stderr()))
            .to_owned();
        Self { child, addr }
    }

    fn get(&self, path: &str, extra: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(&self.addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {}\r\n{extra}Connection: close\r\n\r\n",
            self.addr
        )
        .unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        let status = answer
            .split(' ')
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);
        let body = answer
            .split_once("\r\n\r\n")
            .map_or("", |(_, body)| body)
            .to_owned();
        (status, body)
    }

    /// The snapshot once the first sweep landed (`503` until then).
    fn first_snapshot(&self, extra: &str) -> serde_json::Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let (status, body) = self.get("/v1/snapshot", extra);
            if status == 200 {
                return serde_json::from_str(&body).unwrap();
            }
            assert_eq!(status, 503, "{body}");
            assert!(Instant::now() < deadline, "no reading in time");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn stop(mut self, signal: &str) -> (ExitStatus, String) {
        let killed = Command::new("kill")
            .args([signal, &self.child.id().to_string()])
            .status()
            .unwrap();
        assert!(killed.success());
        (wait(&mut self.child), self.addr.clone())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn wait(child: &mut Child) -> ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < deadline, "did not exit in time");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn run(sandbox: &Sandbox, args: &[&str]) -> ExitStatus {
    let mut child = sandbox.command(args).spawn().unwrap();
    wait(&mut child)
}

fn arg(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn serves_reads_without_queries_and_exits_0_on_sigterm() {
    let sandbox = Sandbox::new("healthy");
    let config = sandbox.config("dev");
    let token = sandbox.file("token", &format!("{TOKEN}\n"));
    let server = Server::start(
        &sandbox,
        &[
            "--config",
            arg(&config),
            "--serve",
            "--port",
            "0",
            "--token-file",
            arg(&token),
        ],
    );
    let bearer = format!("Authorization: Bearer {TOKEN}\r\n");
    let snapshot = server.first_snapshot(&bearer);
    assert_eq!(snapshot["schema"], 1);
    assert_eq!(snapshot["accounts"][0]["key"], "aws_bedrock:Bedrock");
    assert_eq!(snapshot["accounts"][0]["report"]["status"], "ready");
    // The token file is binding: without it, or with another one, 401.
    assert_eq!(server.get("/v1/snapshot", "").0, 401);
    assert_eq!(
        server
            .get(
                "/v1/limits",
                "Authorization: Bearer cuotai_another-token-0123456789\r\n"
            )
            .0,
        401
    );

    let queries = sandbox.aws_calls();
    assert_eq!(queries, 1, "one first sweep");
    for _ in 0..100 {
        assert_eq!(server.get("/v1/snapshot", &bearer).0, 200);
    }
    assert_eq!(sandbox.aws_calls(), queries, "a read never queries");

    let (status, addr) = server.stop("-TERM");
    assert_eq!(status.code(), Some(0), "{}", sandbox.stderr());
    assert!(TcpStream::connect(addr).is_err(), "the port is closed");
    assert!(
        !sandbox.stderr().contains(TOKEN),
        "the log never has the token"
    );
    // INT-04: the token file was written with the default permissions
    // (0644 here), so the run warned that others can read it.
    assert!(
        sandbox
            .stderr()
            .contains("other users of this computer can read the token file"),
        "{}",
        sandbox.stderr()
    );
}

/// INT-04: without `--token-file`, `--serve` creates a random token in a
/// private file of the data directory, says where, requires it (a local
/// client without it gets `401`, `/health` included) and reuses it on the
/// next start.
#[test]
fn requires_a_generated_token_by_default() {
    let sandbox = Sandbox::new("default-token");
    let config = sandbox.config("dev");
    let args = ["--config", arg(&config), "--serve", "--port", "0"];
    let server = Server::start(&sandbox, &args);
    let stderr = sandbox.stderr();
    let path = stderr
        .split("Access token in ")
        .nth(1)
        .and_then(|rest| rest.split(" (only your user").next())
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("no token notice: {stderr}"));
    assert!(path.starts_with(sandbox.0.join("home")), "{path:?}");
    let token = fs::read_to_string(&path).unwrap();
    let token = token.trim();
    assert!(token.starts_with("cuotai_"), "a generated token");
    assert!(!stderr.contains(token), "the token is never printed");
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(path.parent().unwrap()), 0o700);

    assert_eq!(server.get("/health", "").0, 401);
    assert_eq!(server.get("/v1/snapshot", "").0, 401);
    let bearer = format!("Authorization: Bearer {token}\r\n");
    assert_eq!(server.get("/health", &bearer).0, 200);
    let snapshot = server.first_snapshot(&bearer);
    assert_eq!(snapshot["accounts"][0]["report"]["status"], "ready");
    let (status, _) = server.stop("-TERM");
    assert_eq!(status.code(), Some(0), "{}", sandbox.stderr());

    // The next start reuses the same token.
    let server = Server::start(&sandbox, &args);
    assert_eq!(fs::read_to_string(&path).unwrap().trim(), token);
    assert_eq!(server.get("/health", &bearer).0, 200);
    assert_eq!(server.get("/health", "").0, 401);
}

/// PRD 0031 RF10: a signal stop exits 0 whatever the accounts' state;
/// `--strict-exit` keeps the 2 of "no account was healthy".
fn failing_serve_exit(strict: bool) -> Option<i32> {
    let sandbox = Sandbox::new(if strict { "failing-strict" } else { "failing" });
    let config = sandbox.config("broken");
    // INT-04: `--no-token` is the explicit opt-out, with a warning.
    let mut args = vec![
        "--config",
        arg(&config),
        "--serve",
        "--port",
        "0",
        "--no-token",
    ];
    if strict {
        args.push("--strict-exit");
    }
    let server = Server::start(&sandbox, &args);
    assert!(
        sandbox
            .stderr()
            .contains("without a token, any user or program on this computer"),
        "{}",
        sandbox.stderr()
    );
    // The failed reading is served (it is the last known state).
    let snapshot = server.first_snapshot("");
    assert_eq!(snapshot["accounts"][0]["report"]["status"], "error");
    let (status, _) = server.stop("-INT");
    status.code()
}

#[test]
fn exits_0_on_sigint_even_when_no_account_was_healthy() {
    assert_eq!(failing_serve_exit(false), Some(0));
}

#[test]
fn strict_exit_keeps_2_when_no_account_was_healthy() {
    assert_eq!(failing_serve_exit(true), Some(2));
}

#[test]
fn local_errors_exit_1() {
    let sandbox = Sandbox::new("errors");
    let config = sandbox.config("dev");
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port().to_string();
    // An explicit port is binding: taken means an error, not another port.
    let status = run(
        &sandbox,
        &["--config", arg(&config), "--serve", "--port", &port],
    );
    assert_eq!(status.code(), Some(1));
    // PRD 0031 RF9.1: it names the port and suggests `--port`.
    assert!(sandbox.stderr().contains(&port), "{}", sandbox.stderr());
    assert!(sandbox.stderr().contains("--port"), "{}", sandbox.stderr());

    let short = sandbox.file("short-token", "short\n");
    let missing = sandbox.0.join("missing-token");
    let missing_config = sandbox.0.join("missing.toml");
    for args in [
        vec![
            "--config",
            arg(&config),
            "--serve",
            "--token-file",
            arg(&short),
        ],
        vec![
            "--config",
            arg(&config),
            "--serve",
            "--token-file",
            arg(&missing),
        ],
        vec!["--config", arg(&missing_config), "--serve"],
        vec!["--config", arg(&config), "--serve", "--port", "65536"],
        vec!["--config", arg(&config), "--port", "6738"],
        vec!["--config", arg(&config), "--no-token"],
        vec!["--config", arg(&config), "--strict-exit"],
        vec![
            "--config",
            arg(&config),
            "--serve",
            "--no-token",
            "--token-file",
            arg(&short),
        ],
    ] {
        assert_eq!(run(&sandbox, &args).code(), Some(1), "{args:?}");
    }
    assert_eq!(sandbox.aws_calls(), 0, "no query before a local error");
}

/// PRD 0031 RF11: `/v1/stats` exists only with `--serve-stats` and a token;
/// it answers from the local history and never queries a provider.
#[test]
fn stats_route_needs_its_own_flag_and_a_token() {
    let sandbox = Sandbox::new("stats");
    let config = sandbox.config("dev");
    let token = sandbox.file("token", &format!("{TOKEN}\n"));
    let bearer = format!("Authorization: Bearer {TOKEN}\r\n");

    let plain = Server::start(
        &sandbox,
        &[
            "--config",
            arg(&config),
            "--serve",
            "--port",
            "0",
            "--token-file",
            arg(&token),
        ],
    );
    assert_eq!(plain.get("/v1/stats", &bearer).0, 404, "off by default");
    drop(plain);

    let server = Server::start(
        &sandbox,
        &[
            "--config",
            arg(&config),
            "--serve",
            "--serve-stats",
            "--port",
            "0",
            "--token-file",
            arg(&token),
        ],
    );
    server.first_snapshot(&bearer);
    assert_eq!(server.get("/v1/stats", "").0, 401, "a token is required");
    let queries = sandbox.aws_calls();
    let (status, body) = server.get("/v1/stats?range=7d", &bearer);
    assert_eq!(status, 200, "{body}");
    let stats: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(stats["schema"], 1);
    assert_eq!(stats["range"], "7d");
    assert_eq!(stats["accounts"][0]["name"], "Bedrock");
    assert_eq!(stats["daily"].as_array().unwrap().len(), 7);
    assert_eq!(server.get("/v1/stats?range=1y", &bearer).0, 400);
    assert_eq!(sandbox.aws_calls(), queries, "statistics never query");
    let (status, _) = server.stop("-TERM");
    assert_eq!(status.code(), Some(0), "{}", sandbox.stderr());

    // Without a token there is nothing to guard it with: refused up front.
    let mut child = sandbox
        .command(&[
            "--config",
            arg(&config),
            "--serve",
            "--serve-stats",
            "--no-token",
        ])
        .spawn()
        .unwrap();
    assert_eq!(wait(&mut child).code(), Some(1));
}

/// PRD 0031 RF12: `SIGHUP` rereads the TOML: an added account appears in
/// the snapshot and `/v1/limits` while the unchanged one is not queried
/// again, and an invalid file keeps serving the previous configuration.
#[test]
fn sighup_reloads_the_configuration_and_keeps_serving_on_errors() {
    let sandbox = Sandbox::new("reload");
    let config = sandbox.config("dev");
    let token = sandbox.file("token", &format!("{TOKEN}\n"));
    let server = Server::start(
        &sandbox,
        &[
            "--config",
            arg(&config),
            "--serve",
            "--port",
            "0",
            "--token-file",
            arg(&token),
        ],
    );
    let bearer = format!("Authorization: Bearer {TOKEN}\r\n");
    server.first_snapshot(&bearer);
    assert_eq!(sandbox.aws_calls(), 1);
    let hup = |server: &Server| {
        let sent = Command::new("kill")
            .args(["-HUP", &server.child.id().to_string()])
            .status()
            .unwrap();
        assert!(sent.success());
    };
    let accounts = |server: &Server| -> Vec<String> {
        let (status, body) = server.get("/v1/snapshot", &bearer);
        if status != 200 {
            return Vec::new();
        }
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        value["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["key"].as_str().unwrap_or_default().to_owned())
            .collect()
    };

    fs::write(
        &config,
        "[[accounts]]\nname = \"Bedrock\"\nprovider = \"aws_bedrock\"\nparams = { profile = \"dev\" }\n\n\
         [[accounts]]\nname = \"Second\"\nprovider = \"aws_bedrock\"\nparams = { profile = \"two\" }\n",
    )
    .unwrap();
    hup(&server);
    let deadline = Instant::now() + Duration::from_secs(20);
    while accounts(&server).len() != 2 || sandbox.aws_calls() < 2 {
        assert!(Instant::now() < deadline, "no reload: {}", sandbox.stderr());
        std::thread::sleep(Duration::from_millis(100));
    }
    let (status, limits) = server.get("/v1/limits", &bearer);
    assert_eq!(status, 200);
    assert!(limits.contains("Second"), "{limits}");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(sandbox.aws_calls(), 2, "only the new account was queried");
    assert!(sandbox.stderr().contains("configuração recarregada"));

    fs::write(&config, "[[accounts]\nname = ").unwrap();
    hup(&server);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !sandbox.stderr().contains("configuração não recarregada") {
        assert!(Instant::now() < deadline, "{}", sandbox.stderr());
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        accounts(&server).len(),
        2,
        "the previous configuration stays"
    );

    let (status, _) = server.stop("-TERM");
    assert_eq!(status.code(), Some(0), "{}", sandbox.stderr());
}
