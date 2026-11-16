//! PRD 0034 RF3.1: `--help` is written in English, whatever the locale;
//! RF3.2: CLI messages follow the interface language.

use std::process::Command;

fn run(lang: &str, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_cuotai"))
        .args(args)
        .env("CUOTAI_LANG", lang)
        .output()
        .expect("run cuotai")
}

/// Every language of the core registry (PRD 0034 re-validation V-14), so a
/// new language cannot localize `--help` unnoticed. clap's own argument
/// errors stay English too, by decision (PRD 0034 §13).
#[test]
fn help_is_english_in_every_locale() {
    assert!(cuotai_core::text::LOCALES.len() >= 14);
    for lang in cuotai_core::text::LOCALES.iter().map(|row| row.code) {
        let output = run(lang, &["--help"]);
        assert!(output.status.success(), "{lang}: {output:?}");
        let help = String::from_utf8(output.stdout).expect("UTF-8");
        assert!(
            help.contains("Queries once, without opening the TUI"),
            "{help}"
        );
        assert!(help.contains("<ACCOUNT>"), "{help}");
        let markers = cuotai_core::text::portuguese_markers(&help);
        assert!(markers.is_empty(), "{lang}: {markers:?}\n{help}");
    }
}

/// PRD 0013 re-validation R-02: the help follows the behavior. SIGHUP
/// reloads `--watch` (PRD 0031 RF12) and `--hooks` names every variable a
/// hook receives (the core's list).
#[test]
fn the_watch_and_hooks_help_match_the_behavior() {
    let output = run("en-US", &["--help"]);
    let help = String::from_utf8(output.stdout).expect("UTF-8");
    let flat = help.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(!flat.contains("SIGHUP stop"), "{flat}");
    assert!(flat.contains("SIGHUP rereads the configuration"), "{flat}");
    for name in cuotai_core::hooks::GENERATED_ENV {
        assert!(flat.contains(name), "{name} missing from --hooks help");
    }
}

#[test]
fn argument_errors_follow_the_language() {
    let english = run("en-US", &["--days", "7"]);
    let stderr = String::from_utf8_lossy(&english.stderr);
    assert!(stderr.contains("--days only works with"), "{stderr}");
    let portuguese = run("pt-BR", &["--days", "7"]);
    let stderr = String::from_utf8_lossy(&portuguese.stderr);
    assert!(stderr.contains("--days só vale com"), "{stderr}");
    let spanish = run("es", &["--days", "7"]);
    let stderr = String::from_utf8_lossy(&spanish.stderr);
    assert!(stderr.contains("--days solo vale con"), "{stderr}");
    let italian = run("it", &["--days", "7"]);
    let stderr = String::from_utf8_lossy(&italian.stderr);
    assert!(stderr.contains("--days vale solo con"), "{stderr}");
}

/// PRD 0031 re-validation V-11: the help carries no PRD prefix, `--serve`
/// names SIGHUP, and `--reset-lead` is bounded and needs `--watch`.
#[test]
fn the_status_line_serve_and_reset_lead_help_match_the_behavior() {
    let output = run("en-US", &["--help"]);
    let help = String::from_utf8(output.stdout).expect("UTF-8");
    let flat = help.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(!flat.contains("PRD 0031 RF2:"), "{flat}");
    assert!(flat.contains("SIGHUP rereads the TOML"), "{flat}");
    assert!(flat.contains("MIN, 0 to 1440"), "{flat}");
    assert!(!flat.contains("[default: 15]"), "{flat}");
    // A missing config: were the arguments accepted, nothing is queried.
    let config = std::env::temp_dir().join("cuotai-help-no-such-dir/none.toml");
    let config = config.to_str().unwrap();
    for args in [
        &["--config", config, "--once", "--reset-lead", "5"][..],
        &["--config", config, "--watch", "--reset-lead", "1441"][..],
    ] {
        let output = run("en-US", args);
        assert_ne!(output.status.code(), Some(0), "{args:?}: {output:?}");
    }
}
