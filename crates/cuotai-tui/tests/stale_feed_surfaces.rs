//! PRD 0040 re-validation H-1: an agent-fed reading Cuotai read just now
//! but Claude Code wrote 2 hours ago is old on every surface the binary
//! prints: the `--once` heading, `--status-line` (exit 2), and the
//! `--json-v1` export (`freshness`, `reading_age_seconds`). The dashboard
//! card and details, the widget projection and the menu-bar views have
//! their own tests beside their code (`dashboard.rs`, `widget_snapshot.rs`,
//! `tray_tests.rs`), as does the desktop (`scripts/test-stale-feed-ui.mjs`).
//!
//! Sandboxed: a temporary `HOME` and data folder, no provider queries (the
//! account is answered by the feed file alone), status pages off.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use chrono::{TimeDelta, Utc};

struct Sandbox {
    _root: tempfile::TempDir,
    dir: PathBuf,
}

impl Sandbox {
    /// A Claude account answered by the Claude Code feed, whose only
    /// reading was written 2 hours ago.
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().to_path_buf();
        for sub in ["home", "data/feed/claude_code", "bin"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        std::fs::write(
            dir.join("cuotai.toml"),
            "[[accounts]]\nid = \"acc_fed\"\nname = \"Fed\"\nprovider = \"claude\"\nparams = { query_mode = \"claude_code\" }\n",
        )
        .unwrap();
        let now = Utc::now();
        let reading = serde_json::json!({
            "schema": 1,
            "source": "claude_code",
            "written_at": now - TimeDelta::hours(2),
            "session": { "digest": "0123456789abcdef" },
            "windows": [
                { "id": "five_hour", "used_pct": 23.0, "resets_at": (now + TimeDelta::hours(3)).timestamp() },
                { "id": "seven_day", "used_pct": 41.0, "resets_at": (now + TimeDelta::days(3)).timestamp() }
            ]
        });
        std::fs::write(
            dir.join("data/feed/claude_code/0123456789abcdef.json"),
            reading.to_string(),
        )
        .unwrap();
        Self { _root: root, dir }
    }

    fn run(&self, lang: &str, args: &[&str]) -> Output {
        let home = self.dir.join("home");
        let data = self.dir.join("data");
        Command::new(env!("CARGO_BIN_EXE_cuotai"))
            .args(["--config"])
            .arg(self.dir.join("cuotai.toml"))
            .args(["--no-status", "--dev-credentials-sqlite"])
            .arg(self.dir.join("dev.sqlite3"))
            .args(args)
            .env("HOME", &home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("CUOTAI_DATA_DIR", &data)
            .env("CUOTAI_FEED_DIR", data.join("feed"))
            .env("CUOTAI_USER_DIR", home.join("user"))
            .env("CUOTAI_METERED_DIR", home.join("metered"))
            .env("CUOTAI_LANG", lang)
            .env("BROWSER", "true")
            .env("PATH", path(&self.dir.join("bin")))
            .env_remove("CUOTAI_CONFIG")
            .env_remove("NO_COLOR")
            .stdin(Stdio::null())
            .output()
            .expect("run cuotai")
    }
}

impl Sandbox {
    /// `--statusline-claude` (or another agent-side command) with `input`
    /// on standard input.
    fn run_agent(&self, lang: &str, args: &[&str], input: &[u8]) -> Output {
        use std::io::Write;
        let home = self.dir.join("home");
        let data = self.dir.join("data");
        let mut child = Command::new(env!("CARGO_BIN_EXE_cuotai"))
            .args(args)
            .args(["--config"])
            .arg(self.dir.join("cuotai.toml"))
            .env("HOME", &home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("CUOTAI_DATA_DIR", &data)
            .env("CUOTAI_FEED_DIR", data.join("feed"))
            .env("CUOTAI_USER_DIR", home.join("user"))
            .env("CUOTAI_METERED_DIR", home.join("metered"))
            .env("CUOTAI_LANG", lang)
            .env("BROWSER", "true")
            .env("PATH", path(&self.dir.join("bin")))
            .env_remove("CUOTAI_CONFIG")
            .env_remove("NO_COLOR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run cuotai");
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }
}

/// A new Claude Code window that has not reported its limits yet.
const WINDOWLESS: &[u8] = br#"{"session_id":"new-window","context_window":{"used_percentage":5}}"#;

/// `PATH` with the sandbox's `bin` first (no `open`/`xdg-open` there is
/// ever needed; nothing here opens a browser).
fn path(bin: &Path) -> String {
    format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn once_names_the_reading_stale_with_its_own_age() {
    let sandbox = Sandbox::new();
    // (language, the age words of 2 h, "Up to date", "now")
    for (lang, two_hours, current, now) in [
        ("en-US", "Stale · read 2 h ago", "Up to date", " now"),
        (
            "pt-BR",
            "Desatualizada · lido há 2\u{a0}h",
            "Atualizado",
            "agora",
        ),
    ] {
        let output = sandbox.run(lang, &["--once"]);
        assert_eq!(output.status.code(), Some(0), "{lang}: {output:?}");
        let out = text(&output);
        let heading = out
            .lines()
            .find(|line| line.starts_with("Fed / Claude"))
            .unwrap_or_else(|| panic!("{lang}: no heading in {out}"));
        assert!(heading.contains(two_hours), "{lang}: {heading}");
        assert!(!heading.contains(current), "{lang}: {heading}");
        assert!(!heading.contains(now), "{lang}: {heading}");
    }
}

#[test]
fn the_status_line_marks_it_and_exits_2() {
    let sandbox = Sandbox::new();
    let output = sandbox.run("en-US", &["--status-line"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let line = text(&output);
    assert!(line.contains("59%"), "{line}");
    // Re-validation V-04: its age in parentheses, never a bare dot.
    assert!(line.contains("59% (2\u{a0}h)"), "{line}");
    let output = sandbox.run("en-US", &["--status-line", "--account", "acc_fed"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
}

#[test]
fn the_export_says_stale_and_gives_the_reading_age() {
    let sandbox = Sandbox::new();
    let output = sandbox.run("en-US", &["--json-v1"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let export: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let account = &export["accounts"][0];
    assert_eq!(account["freshness"], "stale");
    let reading = account["reading_age_seconds"].as_u64().unwrap();
    assert!((7_190..7_400).contains(&reading), "{reading}");
    // `age_seconds` stays the age of the query (read just now).
    assert!(account["age_seconds"].as_u64().unwrap() < 120);
}

/// Re-validation V-04/V-05: the windows of the status line, written 2 h
/// ago (longer than the account's interval), end with their age in the
/// reader's words, never a bare dot.
#[test]
fn statusline_claude_ages_old_windows() {
    let sandbox = Sandbox::new();
    let output = sandbox.run_agent("en-US", &["--statusline-claude"], WINDOWLESS);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let line = text(&output);
    assert!(line.contains("Wk 41%"), "{line}");
    assert!(line.contains("(2\u{a0}h)"), "{line}");
    let output = sandbox.run_agent("de", &["--statusline-claude"], WINDOWLESS);
    let line = text(&output);
    assert!(line.contains("(2\u{a0}Std.)"), "{line}");
}

/// Re-validation V-03: `--statusline-claude --account` (and `--feed
/// --print`) take an id, a key or a unique name like `--status-line`; an
/// unknown value is reported on stderr with exit 1.
#[test]
fn statusline_claude_account_resolves_like_the_status_line() {
    let sandbox = Sandbox::new();
    for value in ["acc_fed", "claude:Fed", "Fed"] {
        for command in [
            &["--statusline-claude"][..],
            &["--feed", "claude-statusline", "--print"][..],
        ] {
            let args: Vec<&str> = command
                .iter()
                .copied()
                .chain(["--account", value])
                .collect();
            let output = sandbox.run_agent("en-US", &args, WINDOWLESS);
            assert_eq!(output.status.code(), Some(0), "{value}: {output:?}");
            assert!(text(&output).contains("Wk 41%"), "{value}: {output:?}");
        }
    }
    for command in [
        &["--statusline-claude"][..],
        &["--feed", "claude-statusline", "--print"][..],
    ] {
        let args: Vec<&str> = command
            .iter()
            .copied()
            .chain(["--account", "Fde"])
            .collect();
        let output = sandbox.run_agent("en-US", &args, WINDOWLESS);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("--account Fde") && stderr.contains("acc_fed (Fed)"),
            "{stderr}"
        );
    }
}
