//! PRD 0031 RF10 on Windows: `--watch` and `--serve` stop on Ctrl+Break and
//! exit 0 even when every account ends in failure, or 2 with `--strict-exit`.
//! The child runs in its own process group (so the event reaches only it) and
//! receives `CTRL_BREAK_EVENT` through `GenerateConsoleCtrlEvent`. The fixture
//! names only never-defined credential variables: no network is involved.
//! Needs a console, which GitHub's Windows runners provide; the Unix twin is
//! `watch.rs`.

#![cfg(windows)]

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const CTRL_BREAK_EVENT: u32 = 1;

extern "system" {
    fn GenerateConsoleCtrlEvent(event: u32, process_group_id: u32) -> i32;
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/smoke.toml")
}

fn exit_after_ctrl_break(args: &[&str]) -> Option<i32> {
    let config = fixture();
    let mut child: Child = Command::new(env!("CARGO_BIN_EXE_cuotai"))
        .args(["--config", config.to_str().unwrap()])
        .args(args)
        .env_remove("CUOTAI_CONFIG")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .unwrap();
    // Give the first (all-failing) reading and the signal handlers time to start.
    std::thread::sleep(Duration::from_secs(3));
    assert!(
        child.try_wait().unwrap().is_none(),
        "exited before the signal"
    );
    let sent = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child.id()) };
    assert_ne!(sent, 0, "GenerateConsoleCtrlEvent failed");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("did not exit after Ctrl+Break");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn watch_exits_0_on_ctrl_break_with_failing_accounts() {
    assert_eq!(exit_after_ctrl_break(&["--watch"]), Some(0));
}

#[test]
fn watch_strict_exit_keeps_2_on_ctrl_break() {
    assert_eq!(
        exit_after_ctrl_break(&["--watch", "--strict-exit"]),
        Some(2)
    );
}

#[test]
fn serve_exits_0_on_ctrl_break_with_failing_accounts() {
    assert_eq!(
        exit_after_ctrl_break(&["--serve", "--port", "0", "--no-token"]),
        Some(0)
    );
}
