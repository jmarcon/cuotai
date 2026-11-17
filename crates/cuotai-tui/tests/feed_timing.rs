//! PRD 0040 RF2.3 and RF5.4 acceptance: the wall time of the agent-feed
//! commands, measured on the built binary in a release build.
//!
//! - `--feed claude-statusline` (with and without `--print`) and
//!   `--statusline-claude`: under 50 ms on a warm run.
//! - `--statusline-claude --from-server` against a loopback server that
//!   never answers: under 150 ms, falling back to the history.
//!
//! The budgets only mean something in a release build on a quiet machine,
//! so the test runs only with `CUOTAI_TIMING=1` and refuses a debug build:
//! `CUOTAI_TIMING=1 cargo test --release -p cuotai-tui --test feed_timing`.
//! CI runs it in the `release-profile` job. Every run is sandboxed: its own
//! home, data, user, feed and config locations under a temporary folder,
//! no proxy, and no provider is ever contacted.

use std::io::Write;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// RF2.3 and RF5.4: without a server.
const LOCAL_BUDGET: Duration = Duration::from_millis(50);
/// RF5.4: with `--from-server` on loopback.
const SERVER_BUDGET: Duration = Duration::from_millis(150);
/// Measured runs per command, after one warm-up run; the median is checked.
const RUNS: usize = 9;

fn enabled() -> bool {
    std::env::var("CUOTAI_TIMING").is_ok_and(|value| value == "1")
}

/// Every variable that could point a run at the owner's files or a proxy.
const CLEARED: &[&str] = &[
    "CUOTAI_CONFIG",
    "CUOTAI_DATA_DIR",
    "CUOTAI_USER_DIR",
    "CUOTAI_METERED_DIR",
    "CUOTAI_FEED_DIR",
    "CUOTAI_LANG",
    "CODEX_HOME",
    "CLAUDE_CONFIG_DIR",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "NO_COLOR",
];

/// One run of the binary with `args` and `input` on stdin, in the sandbox
/// below `root`; its wall time from spawn to exit.
fn timed(root: &Path, args: &[&str], input: &[u8]) -> Duration {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cuotai"));
    for name in CLEARED {
        command.env_remove(name);
    }
    let started = Instant::now();
    let mut child = command
        .args(args)
        .env("HOME", root.join("home"))
        .env("USERPROFILE", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("XDG_STATE_HOME", root.join("xdg-state"))
        .env("APPDATA", root.join("appdata"))
        .env("LOCALAPPDATA", root.join("localappdata"))
        .env("CUOTAI_DATA_DIR", root.join("data"))
        .env("CUOTAI_USER_DIR", root.join("user"))
        .env("CUOTAI_METERED_DIR", root.join("metered"))
        .env("CUOTAI_FEED_DIR", root.join("feed"))
        .env("CUOTAI_CONFIG", root.join("missing.toml"))
        .env("CODEX_HOME", root.join("codex"))
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("CUOTAI_LANG", "en-US")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run the built binary");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait for the binary");
    let elapsed = started.elapsed();
    assert!(
        output.status.success(),
        "{args:?} exited with {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    elapsed
}

/// The median wall time of `args` over [`RUNS`] runs after a warm-up run.
fn median(root: &Path, args: &[&str], input: &[u8]) -> Duration {
    timed(root, args, input);
    let mut times: Vec<Duration> = (0..RUNS).map(|_| timed(root, args, input)).collect();
    times.sort();
    let median = times[RUNS / 2];
    println!(
        "{args:?}: median {median:?}, min {:?}, max {:?}",
        times[0],
        times[RUNS - 1]
    );
    median
}

/// The documented status-line object with both Claude windows (RF2.1).
fn input() -> Vec<u8> {
    let reset = chrono::Utc::now().timestamp() + 3_600;
    format!(
        r#"{{"session_id":"s-1","model":{{"id":"claude-opus-5-5","display_name":"Opus"}},"context_window":{{"used_percentage":8,"remaining_percentage":92}},"cost":{{"total_cost_usd":0.42}},"rate_limits":{{"five_hour":{{"used_percentage":23,"resets_at":{reset}}},"seven_day":{{"used_percentage":41}}}}}}"#
    )
    .into_bytes()
}

/// A loopback server that accepts every connection and never answers, so
/// the status line must give up within its own budget.
fn silent_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
    let address = listener.local_addr().expect("local address");
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming().flatten() {
            held.push(stream);
        }
    });
    format!("http://{address}")
}

#[test]
fn the_feed_commands_finish_within_their_budgets() {
    if !enabled() {
        println!("skipped: set CUOTAI_TIMING=1 and build with --release to measure");
        return;
    }
    if cfg!(debug_assertions) {
        panic!("the budgets apply to a release build: run with --release");
    }
    let sandbox = tempfile::tempdir().expect("sandbox");
    let root = sandbox.path();
    let input = input();

    let mut over = Vec::new();
    let mut check = |args: &[&str], budget: Duration| {
        let measured = median(root, args, &input);
        if measured >= budget {
            over.push(format!("{args:?}: {measured:?} (budget {budget:?})"));
        }
    };
    // RF2.3: the writer, silent and with the line.
    check(&["--feed", "claude-statusline"], LOCAL_BUDGET);
    check(&["--feed", "claude-statusline", "--print"], LOCAL_BUDGET);
    // RF5.4: the status line from the history.
    check(&["--statusline-claude"], LOCAL_BUDGET);
    // RF5.4: a server that never answers does not delay the agent.
    let server = silent_server();
    check(
        &["--statusline-claude", "--from-server", server.as_str()],
        SERVER_BUDGET,
    );
    assert!(over.is_empty(), "over budget:\n{}", over.join("\n"));
}
