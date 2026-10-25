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

#[test]
fn help_is_english_in_every_locale() {
    for lang in ["pt-BR", "en-US", "es", "it"] {
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
