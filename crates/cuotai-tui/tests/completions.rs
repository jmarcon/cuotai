//! PRD 0031 RF3: `cuotai --completions SHELL` prints a script generated
//! from the command-line definition, without reading any configuration.

use std::process::Command;

fn completions(shell: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_cuotai"))
        .args(["--completions", shell])
        .output()
        .expect("run cuotai")
}

#[test]
fn every_shell_script_is_generated_and_knows_the_flags() {
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let output = completions(shell);
        assert!(output.status.success(), "{shell}: {output:?}");
        assert!(output.stderr.is_empty(), "{shell}: {output:?}");
        let script = String::from_utf8(output.stdout).expect("UTF-8 script");
        assert!(script.contains("cuotai"), "{shell}");
        for flag in ["serve", "watch", "completions", "status-line"] {
            assert!(script.contains(flag), "{shell} should mention --{flag}");
        }
    }
}

#[test]
fn an_unknown_shell_or_a_query_mode_is_a_local_argument_error() {
    assert_eq!(completions("tcsh").status.code(), Some(1));
    let output = Command::new(env!("CARGO_BIN_EXE_cuotai"))
        .args(["--completions", "bash", "--once"])
        .output()
        .expect("run cuotai");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}
