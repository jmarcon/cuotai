//! Every subcommand answers `--help` and rejects an unknown option (PRD 0039
//! F-05): a misspelt option must never pass silently.

use std::process::Command;

fn xtask(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .env_remove("GITHUB_ACTIONS")
        .output()
        .expect("run xtask")
}

/// The subcommands the top-level usage lists (two-space headers).
fn subcommands() -> Vec<String> {
    let usage = String::from_utf8(xtask(&["help"]).stdout).unwrap();
    let body = usage.split_once("SUBCOMMANDS:\n").unwrap().1;
    body.lines()
        .filter(|line| line.starts_with("  ") && line.as_bytes()[2].is_ascii_lowercase())
        .map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            if words[0] == "release" {
                format!("release {}", words[1])
            } else {
                words[0].to_string()
            }
        })
        .collect()
}

#[test]
fn every_subcommand_answers_help() {
    let commands = subcommands();
    assert!(commands.len() >= 20, "{commands:?}");
    for command in &commands {
        let mut args: Vec<&str> = command.split(' ').collect();
        args.push("--help");
        let out = xtask(&args);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{command} --help failed: {out:?}");
        assert!(
            stdout.contains(&format!("cargo xtask {command}")),
            "{command}: {stdout}"
        );
    }
}

#[test]
fn every_subcommand_rejects_an_unknown_option() {
    for command in subcommands() {
        let mut args: Vec<&str> = command.split(' ').collect();
        args.extend(["--bogus", "x"]);
        let out = xtask(&args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{command} accepted --bogus");
        assert!(stderr.contains("--bogus"), "{command}: {stderr}");
        assert!(
            !stderr.contains("unknown subcommand"),
            "{command}: {stderr}"
        );
    }
}

#[test]
fn a_misspelt_tag_option_fails_instead_of_skipping_the_tag_check() {
    let out = xtask(&["release-check", "--tga", "v9.9.9"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(
        stderr.contains("release-check does not accept --tga: unknown option"),
        "{stderr}"
    );
    let out = xtask(&["changelog-gate", "--bse", "v0.1.0"]);
    assert!(!out.status.success());
}

#[test]
fn release_help_lists_every_action() {
    let out = xtask(&["release", "--help"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    for action in ["prepare", "edit", "tag", "ci-proof"] {
        assert!(stdout.contains(&format!("release {action}")), "{stdout}");
    }
}
