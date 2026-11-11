//! Guard for the shell scripts under `scripts/`: a script with `pipefail`
//! must not pipe into a reader that exits early (`grep -q`, `grep -m`,
//! `head`). The reader closes the pipe on the first match, the writer dies of
//! SIGPIPE, and `pipefail` turns a match into a failure. That false negative
//! failed the v0.85.2 macOS smoke (`nm | grep -q`). Capture the output first,
//! then grep the variable with a here-string.

use std::path::{Path, PathBuf};

fn shell_scripts(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            shell_scripts(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "sh") {
            out.push(path);
        }
    }
}

/// True when the command after a pipe is a reader that may stop early.
fn early_exit_reader(command: &str) -> bool {
    let mut words = command.split_whitespace();
    match words.next() {
        Some("head") => true,
        Some("grep") => words.take_while(|w| w.starts_with('-')).any(|flag| {
            matches!(flag, "--quiet" | "--silent")
                || flag.starts_with("--max-count")
                || (!flag.starts_with("--") && (flag.contains('q') || flag.contains('m')))
        }),
        _ => false,
    }
}

/// The offending lines of one script, as `line: text`.
fn offenders(text: &str) -> Vec<String> {
    if !text.contains("pipefail") {
        return Vec::new();
    }
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with('#'))
        .filter(|(_, line)| {
            // Mask `||` so only real pipes split the line: `a || grep -q` is fine.
            line.replace("||", "\u{0}\u{0}")
                .split('|')
                .skip(1)
                .any(early_exit_reader)
        })
        .map(|(n, line)| format!("{}: {}", n + 1, line.trim()))
        .collect()
}

#[test]
fn pipefail_scripts_do_not_pipe_into_early_exit_readers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts");
    let mut scripts = Vec::new();
    shell_scripts(&root, &mut scripts);
    assert!(
        !scripts.is_empty(),
        "no scripts found under {}",
        root.display()
    );
    let mut failures = Vec::new();
    for script in scripts {
        let text = std::fs::read_to_string(&script).unwrap();
        for line in offenders(&text) {
            failures.push(format!("{}:{line}", script.display()));
        }
    }
    assert!(
        failures.is_empty(),
        "pipe into grep -q/-m or head under pipefail (capture the output, then grep it):\n{}",
        failures.join("\n")
    );
}

#[test]
fn detector_flags_early_exit_pipes_only() {
    let script = |line: &str| format!("set -euo pipefail\n{line}\n");
    for bad in [
        "nm -u bin | grep -q '^_main$' || exit 1",
        "security find-identity | grep -qF \"$id\"",
        "tar --version | grep -q GNU",
        "ls | head -1",
        "cat f | grep -m1 x",
        "cat f | grep --quiet x",
    ] {
        assert_eq!(offenders(&script(bad)).len(), 1, "{bad}");
    }
    for good in [
        "grep -q x <<<\"$out\" || fail",
        "a || grep -q x file",
        "dpkg -L p | grep '^/usr/bin/' || true",
        "# nm | grep -q x",
        "echo a | sort -u",
    ] {
        assert!(offenders(&script(good)).is_empty(), "{good}");
    }
    assert!(offenders("nm | grep -q x\n").is_empty(), "no pipefail");
}
