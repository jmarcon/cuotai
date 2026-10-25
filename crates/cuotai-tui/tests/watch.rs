//! PRD 0013 RF2: `cuotai --watch` as a script runs it: the binary in the
//! background with a fake `bl` (Alibaba Token Plan CLI) as the only
//! provider, stopped with a signal, judged by its streams and exit code.
//! The transitions themselves are covered against `wiremock` with an
//! accelerated clock in `cuotai-core` (`watch` tests); a real reading here
//! is at least 60 s after the previous one.

#![cfg(unix)]

use std::fs;
use std::io::{BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// A private HOME with a fake `bl` that logs every call and answers with
/// the 5 h usage in `usage` (`fail` makes it fail).
struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str, usage: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cuotai-watch-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::create_dir_all(dir.join("home")).unwrap();
        fs::write(dir.join("usage"), usage).unwrap();
        let bl = dir.join("bin/bl");
        fs::write(
            &bl,
            format!(
                r#"#!/bin/sh
echo "$@" >> '{log}'
usage=$(cat '{usage}')
if [ "$usage" = fail ]; then echo 'raw failure secret-marker-7' >&2; exit 1; fi
printf '{{"per5HourPercentage":%s,"per5HourResetTime":%s000}}\n' "$usage" $(( $(date +%s) + 10800 ))
"#,
                log = dir.join("bl-calls.log").display(),
                usage = dir.join("usage").display(),
            ),
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

    fn config(&self) -> String {
        self.0.join("cuotai.toml").to_str().unwrap().to_owned()
    }

    fn calls(&self) -> usize {
        fs::read_to_string(self.0.join("bl-calls.log")).map_or(0, |log| log.lines().count())
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
            .env_remove("CUOTAI_CONFIG")
            .env_remove("CUOTAI_STATUS")
            .env("CUOTAI_LANG", "pt-BR")
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

    /// Wait until `found` holds, or fail with the stderr so far.
    fn until(&self, what: &str, found: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !found(self) {
            assert!(Instant::now() < deadline, "{what}: {}", self.stderr());
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Send `signal`, then the exit status and everything written on stdout.
fn stop(mut child: Child, signal: &str) -> (ExitStatus, String) {
    let killed = Command::new("kill")
        .args([signal, &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "did not exit in time");
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    (status, stdout)
}

fn get(addr: &str, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut answer = String::new();
    BufReader::new(stream).read_to_string(&mut answer).unwrap();
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

/// RF2.5 and RF2.4: `--watch` and `--serve` share one cycle (one query
/// for both), the first reading is only a baseline (nothing on stdout,
/// which belongs to the events, so the address goes to stderr) and SIGTERM
/// ends the run with 0 (SIGHUP reloads since PRD 0031 RF12).
#[test]
fn watch_with_serve_shares_the_cycle_and_exits_0_on_sigterm() {
    let sandbox = Sandbox::new("serve", "0.5");
    let config = sandbox.config();
    let child = sandbox
        .command(&[
            // INT-04: a token is required by default; this test is about the
            // shared cycle, so it opts out explicitly.
            "--config",
            &config,
            "--watch",
            "--jsonl",
            "--serve",
            "--port",
            "0",
            "--no-token",
        ])
        .spawn()
        .unwrap();
    sandbox.until("no address on stderr", |s| {
        s.stderr().contains("servidor local em http://")
    });
    let stderr = sandbox.stderr();
    let addr = stderr
        .lines()
        .find_map(|line| line.split("servidor local em http://").nth(1))
        .unwrap()
        .trim()
        .to_owned();
    let deadline = Instant::now() + Duration::from_secs(20);
    let snapshot: serde_json::Value = loop {
        let (status, body) = get(&addr, "/v1/snapshot");
        if status == 200 {
            break serde_json::from_str(&body).unwrap();
        }
        assert!(Instant::now() < deadline, "no reading in time");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(snapshot["accounts"][0]["report"]["status"], "ready");
    assert_eq!(
        snapshot["accounts"][0]["report"]["metrics"][0]["used"],
        50.0
    );
    assert_eq!(sandbox.calls(), 1, "one query serves both");

    let (status, stdout) = stop(child, "-TERM");
    assert_eq!(status.code(), Some(0), "{}", sandbox.stderr());
    assert_eq!(stdout, "", "a baseline emits no event");
    assert_eq!(sandbox.calls(), 1);
    assert!(sandbox.stderr().contains("acompanhamento encerrado."));
}

/// RF2.4 and PRD 0031 RF10: an account whose latest reading failed still
/// ends a signal stop with 0, or with 2 under `--strict-exit`; the
/// provider's raw failure never reaches the streams.
fn failing_watch_exit(strict: bool) -> Option<i32> {
    let name = if strict { "failing-strict" } else { "failing" };
    let sandbox = Sandbox::new(name, "fail");
    let config = sandbox.config();
    let mut args = vec!["--config", &config, "--watch"];
    if strict {
        args.push("--strict-exit");
    }
    let child = sandbox.command(&args).spawn().unwrap();
    sandbox.until("no query", |s| s.calls() == 1);
    // The reading lands on the next tick (250 ms).
    std::thread::sleep(Duration::from_secs(1));
    let (status, stdout) = stop(child, "-TERM");
    assert_eq!(stdout, "", "a failed first reading is a baseline too");
    assert!(sandbox.stderr().contains("contas em falha"));
    assert!(!sandbox.stderr().contains("secret-marker-7"));
    status.code()
}

#[test]
fn exits_0_on_sigterm_even_when_an_account_ends_in_failure() {
    assert_eq!(failing_watch_exit(false), Some(0));
}

#[test]
fn strict_exit_keeps_2_when_an_account_ends_in_failure() {
    assert_eq!(failing_watch_exit(true), Some(2));
}

/// RF2.1: a short interval is rejected (never adjusted), as are unknown
/// events and the watch flags without `--watch`; nothing is queried.
#[test]
fn local_errors_exit_1_before_any_query() {
    let sandbox = Sandbox::new("errors", "0.5");
    let config = sandbox.config();
    let config = config.as_str();
    for args in [
        vec!["--config", config, "--watch", "--interval", "59"],
        vec!["--config", config, "--watch", "--interval", "86401"],
        vec!["--config", config, "--watch", "--interval", "-1"],
        vec![
            "--config",
            config,
            "--watch",
            "--events",
            "quota_exhausted,nope",
        ],
        vec!["--config", config, "--watch", "--events", ""],
        vec!["--config", config, "--interval", "60"],
        vec!["--config", config, "--jsonl"],
        vec!["--config", config, "--events", "quota_exhausted"],
        vec!["--config", config, "--hooks"],
        vec!["--config", config, "--strict-exit"],
        vec!["--config", config, "--watch", "--once"],
    ] {
        let mut child = sandbox.command(&args).spawn().unwrap();
        assert_eq!(child.wait().unwrap().code(), Some(1), "{args:?}");
    }
    let mut child = sandbox
        .command(&["--config", config, "--watch", "--interval", "30"])
        .spawn()
        .unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(1));
    assert!(
        sandbox.stderr().contains("entre 60 e 86400"),
        "{}",
        sandbox.stderr()
    );
    assert_eq!(sandbox.calls(), 0, "no query before a local error");
}

/// PRD 0013 RF3.2: `--hooks` without a terminal to confirm in lists the
/// exact commands and stops (exit 1) before any query or hook; so do a
/// missing and an invalid `hooks.toml`.
#[test]
fn hooks_need_a_confirmation_before_any_query() {
    let sandbox = Sandbox::new("hooks", "0.5");
    let config = sandbox.config();
    let args = ["--config", config.as_str(), "--watch", "--jsonl", "--hooks"];
    let run = || {
        let mut child = sandbox.command(&args).spawn().unwrap();
        let code = child.wait().unwrap().code();
        let mut stdout = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .unwrap();
        (code, stdout, sandbox.stderr())
    };

    let (code, _, stderr) = run();
    assert_eq!(code, Some(1));
    assert!(stderr.contains("Nenhum arquivo de hooks em"), "{stderr}");

    let hooks = sandbox.0.join("hooks.toml");
    fs::write(
        &hooks,
        "[[hooks]]\nname = \"x\"\nevents = [\"quota_exhausted\"]\ncommand = \"touch x\"\n",
    )
    .unwrap();
    let (code, _, stderr) = run();
    assert_eq!(code, Some(1));
    assert!(
        stderr.contains("hooks.toml inválido na linha 4"),
        "{stderr}"
    );

    let marker = sandbox.0.join("ran");
    fs::write(
        &hooks,
        format!(
            "[[hooks]]\nname = \"mark\"\nevents = [\"quota_exhausted\", \"usage_updated\"]\ncommand = [\"/usr/bin/touch\", \"{}\"]\n",
            marker.display()
        ),
    )
    .unwrap();
    let (code, stdout, stderr) = run();
    assert_eq!(code, Some(1), "{stderr}");
    assert_eq!(stdout, "");
    assert!(
        stderr.contains(&format!(
            "comando: [\"/usr/bin/touch\",\"{}\"]",
            marker.display()
        )),
        "{stderr}"
    );
    assert!(stderr.contains("ainda não foram confirmados"), "{stderr}");
    assert_eq!(sandbox.calls(), 0, "no query before the confirmation");
    assert!(!marker.exists(), "no hook before the confirmation");
}

/// PRD 0023 RF5: `--watch --jsonl` prints `overage_started` when an
/// approved script extension starts billing extra usage and `overage_ended`
/// when it stops. The extension counts its runs in a file outside its
/// folder (a write inside would change the folder hash and revoke the
/// approval), so each real query gets the next scripted amount. A reading
/// is at least 60 s after the previous one: this test takes about two
/// minutes.
#[test]
fn watch_jsonl_prints_overage_started_and_ended() {
    let sandbox = Sandbox::new("overage", "0.5");
    let home = sandbox.0.join("home");
    // `dirs::data_local_dir` under the sandbox HOME.
    let data = if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        home.join("data")
    };
    let root = data.join("cuotai/extensions");
    let folder = root.join("ovg");
    fs::create_dir_all(&folder).unwrap();
    let counter = sandbox.0.join("overage-calls");
    fs::write(
        folder.join("manifest.toml"),
        "schema = 1\nid = \"ovg\"\nname = \"Overage\"\nversion = \"1\"\ncommand = [\"./run.sh\"]\ninterval_seconds = 60\ntimeout_seconds = 10\nenv = []\n",
    )
    .unwrap();
    let script = folder.join("run.sh");
    fs::write(
        &script,
        format!(
            r#"#!/bin/sh
echo x >> '{counter}'
calls=$(wc -l < '{counter}' | tr -d ' ')
case "$calls" in
  1) spent=0;;
  2) spent=2.5;;
  *) spent=0;;
esac
printf '{{"schema":1,"status":"ready","metrics":[{{"label":"Extra usage","used":%s,"limit":10,"remaining":null,"unit":"USD","kind":"spend","billing":"overage"}}],"notes":[]}}\n' "$spent"
"#,
            counter = counter.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
    // The approval a person gives with `--extension-check`, made here
    // because that command needs a terminal.
    let manager = cuotai_core::extensions::management::Manager::at(&root);
    let disclosure = manager.disclosure("ovg").unwrap();
    manager.approve(&disclosure.candidate, true).unwrap();
    fs::write(
        sandbox.0.join("cuotai.toml"),
        "[[accounts]]\nname = \"Ovg\"\nprovider = \"extension\"\nparams = { extension_id = \"ovg\" }\n",
    )
    .unwrap();

    let config = sandbox.config();
    let child = sandbox
        .command(&[
            "--config",
            &config,
            "--watch",
            "--jsonl",
            "--interval",
            "60",
            "--events",
            "overage_started,overage_ended",
        ])
        .spawn()
        .unwrap();
    let runs = || fs::read_to_string(&counter).map_or(0, |log| log.lines().count());
    let deadline = Instant::now() + Duration::from_secs(240);
    while runs() < 3 {
        assert!(
            Instant::now() < deadline,
            "only {} queries: {}",
            runs(),
            sandbox.stderr()
        );
        std::thread::sleep(Duration::from_millis(500));
    }
    // The third reading lands on the next tick (250 ms).
    std::thread::sleep(Duration::from_secs(2));
    let (status, stdout) = stop(child, "-TERM");
    assert_eq!(status.code(), Some(0), "{}", sandbox.stderr());
    let events: Vec<serde_json::Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let names: Vec<&str> = events
        .iter()
        .map(|event| event["event"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["overage_started", "overage_ended"], "{stdout}");
    assert_eq!(events[0]["current"]["spent"], 2.5, "{stdout}");
}
