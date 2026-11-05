//! PRD 0040 RF2 and RF5: `cuotai --feed claude-statusline` stores only the
//! contract fields and prints nothing; `cuotai --statusline-claude` prints
//! one line; neither needs a configuration or touches the network.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn run(data: &Path, args: &[&str], input: &[u8]) -> (Output, Duration) {
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_cuotai"))
        .args(args)
        .env("CUOTAI_DATA_DIR", data)
        .env("CUOTAI_LANG", "en-US")
        .env("CUOTAI_CONFIG", data.join("missing.toml"))
        .env_remove("NO_COLOR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run cuotai");
    child.stdin.take().unwrap().write_all(input).unwrap();
    let output = child.wait_with_output().unwrap();
    (output, started.elapsed())
}

fn data_dir(name: &str) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join(name);
    (root, data)
}

fn input() -> Vec<u8> {
    let reset = chrono::Utc::now().timestamp() + 3_600;
    format!(
        r#"{{"session_id":"s-1","cwd":"/Users/ana/SENSITIVE","transcript_path":"/Users/ana/SENSITIVE.jsonl","model":{{"id":"claude-opus-5-5","display_name":"Opus"}},"context_window":{{"used_percentage":8,"remaining_percentage":92}},"cost":{{"total_cost_usd":0.42}},"rate_limits":{{"five_hour":{{"used_percentage":23,"resets_at":{reset}}},"seven_day":{{"used_percentage":41}}}}}}"#
    )
    .into_bytes()
}

fn feed_files(data: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(data.join("feed/claude_code"))
        .map(|entries| entries.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default()
}

#[test]
fn the_feed_stores_the_contract_fields_and_prints_nothing() {
    let (_root, data) = data_dir("feed");
    let (output, elapsed) = run(&data, &["--feed", "claude-statusline"], &input());
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "prints nothing by default");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let files = feed_files(&data);
    assert_eq!(files.len(), 1);
    let stored = std::fs::read_to_string(&files[0]).unwrap();
    assert!(stored.contains("\"used_pct\":23.0"), "{stored}");
    assert!(
        !stored.contains("SENSITIVE") && !stored.contains("s-1"),
        "{stored}"
    );
    // Generous for a debug build on a shared runner; the release budget
    // is 50 ms (RF2.3).
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
    // Nothing else is written: no app log, no history.
    let entries: Vec<String> = std::fs::read_dir(&data)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries, vec!["feed".to_owned()]);
}

#[test]
fn absent_windows_still_exit_zero_and_malformed_input_exits_one() {
    let (_root, data) = data_dir("absent");
    let (output, _) = run(
        &data,
        &["--feed", "claude-statusline"],
        br#"{"session_id":"x"}"#,
    );
    assert!(output.status.success(), "{output:?}");
    let (output, _) = run(&data, &["--feed", "claude-statusline"], b"not json");
    assert_eq!(output.status.code(), Some(1));
    let big = vec![b' '; 64 * 1024 + 10];
    let (output, _) = run(&data, &["--feed", "claude-statusline"], &big);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn the_status_line_prints_claude_windows_and_context() {
    let (_root, data) = data_dir("line");
    let (output, _) = run(&data, &["--statusline-claude"], &input());
    assert!(output.status.success(), "{output:?}");
    let line = String::from_utf8(output.stdout).unwrap();
    let line = line.trim_end();
    assert!(line.starts_with("5h 23% · Wk 41% ↺ "), "{line}");
    assert!(line.ends_with(" · ctx 8%"), "{line}");
    assert!(
        !line.contains('\u{1b}'),
        "no colour without --color on a pipe"
    );
    let (output, _) = run(
        &data,
        &["--statusline-claude", "--max-width", "14"],
        &input(),
    );
    let line = String::from_utf8(output.stdout).unwrap();
    assert_eq!(line.trim_end().chars().count(), 14, "{line}");
    assert!(line.trim_end().ends_with('…'), "{line}");
    let (output, _) = run(&data, &["--feed", "claude-statusline", "--print"], &input());
    let line = String::from_utf8(output.stdout).unwrap();
    assert!(line.starts_with("5h 23%"), "{line}");
}

#[test]
fn setup_prints_the_settings_snippet() {
    let (_root, data) = data_dir("setup");
    let (output, _) = run(&data, &["--feed", "claude-statusline", "--setup"], b"");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains(r#""command": "cuotai --feed claude-statusline --print""#),
        "{text}"
    );
    assert!(feed_files(&data).is_empty(), "--setup stores nothing");
}
