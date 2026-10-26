//! PRD 0034 RF5: `--json` and `--json-v1` carry English `fallback` texts;
//! `--fallback-locale pt-BR` (or `CUOTAI_FALLBACK_LOCALE`) renders them, and
//! the plain `notes`, in Portuguese with the same ids. An account whose
//! credential variable is unset fails before any network request.

#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const ENGLISH: &str = "Missing credential: CUOTAI_FALLBACK_TEST_UNSET.";
const PORTUGUESE: &str = "Credencial ausente: CUOTAI_FALLBACK_TEST_UNSET";

struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "cuotai-fallback-locale-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("home")).unwrap();
        fs::write(
            dir.join("cuotai.toml"),
            "[[accounts]]\nname = \"Groq\"\nprovider = \"groq\"\ncredential_env = \"CUOTAI_FALLBACK_TEST_UNSET\"\n",
        )
        .unwrap();
        Self(dir)
    }

    fn run(&self, args: &[&str], env: Option<&str>) -> Output {
        let home = self.0.join("home");
        let mut command = Command::new(env!("CARGO_BIN_EXE_cuotai"));
        command
            .args(["--config", self.0.join("cuotai.toml").to_str().unwrap()])
            .args(args)
            .env("HOME", &home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env_remove("CUOTAI_CONFIG")
            .env_remove("CUOTAI_STATUS")
            .env_remove("CUOTAI_FALLBACK_TEST_UNSET")
            .env_remove("CUOTAI_FALLBACK_LOCALE")
            .stdin(Stdio::null());
        if let Some(locale) = env {
            command.env("CUOTAI_FALLBACK_LOCALE", locale);
        }
        command.output().unwrap()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn json(output: &Output) -> serde_json::Value {
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("JSON on stdout")
}

#[test]
fn json_fallbacks_are_english_unless_a_fallback_locale_is_chosen() {
    let sandbox = Sandbox::new("json");
    let english = json(&sandbox.run(&["--json"], None));
    let report = &english["reports"][0];
    assert_eq!(report["notes"][0], ENGLISH);
    assert_eq!(report["notes_l10n"][0]["fallback"], ENGLISH);
    assert_eq!(report["notes_l10n"][0]["id"], "query.missing_credential");

    for portuguese in [
        json(&sandbox.run(&["--json", "--fallback-locale", "pt-BR"], None)),
        json(&sandbox.run(&["--json"], Some("pt-BR"))),
    ] {
        let report = &portuguese["reports"][0];
        assert_eq!(report["notes"][0], PORTUGUESE);
        assert_eq!(report["notes_l10n"][0]["fallback"], PORTUGUESE);
        assert_eq!(report["notes_l10n"][0]["id"], "query.missing_credential");
        assert_eq!(
            report["notes_l10n"][0]["params"],
            english["reports"][0]["notes_l10n"][0]["params"]
        );
    }
}

#[test]
fn json_v1_follows_the_fallback_locale() {
    let sandbox = Sandbox::new("json-v1");
    let english = json(&sandbox.run(&["--json-v1"], None));
    // The export redacts the variable name.
    assert_eq!(
        english["accounts"][0]["report"]["notes"][0],
        "Missing credential: [redacted]."
    );
    let portuguese = json(&sandbox.run(&["--json-v1", "--fallback-locale", "pt-BR"], None));
    assert_eq!(
        portuguese["accounts"][0]["report"]["notes"][0],
        "Credencial ausente: [redacted]"
    );
    assert_eq!(portuguese["schema"], english["schema"]);
}

#[test]
fn an_unsupported_or_misplaced_fallback_locale_is_a_local_error() {
    let sandbox = Sandbox::new("errors");
    for args in [
        &["--json", "--fallback-locale", "xx"][..],
        &["--once", "--fallback-locale", "pt-BR"][..],
    ] {
        let output = sandbox.run(args, None);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
    }
    // en-US is the fallback language itself: accepted, nothing changes.
    let english = json(&sandbox.run(&["--json", "--fallback-locale", "en-US"], None));
    assert_eq!(english["reports"][0]["notes"][0], ENGLISH);
}
