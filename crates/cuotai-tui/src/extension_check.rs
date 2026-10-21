//! PRD 0017 RF2.4: `cuotai --extension-check <id>` runs an approved
//! extension once — through the real runner (PRD 0017 RF3) — and shows the
//! normalized reading or the contract violations (PRD 0017 RF2), never the
//! raw output.
//!
//! An extension that was never approved, or whose manifest or program
//! changed since the approval, is first disclosed (exact command,
//! directory, requested environment variables, interval) and runs only
//! after the typed confirmation (PRD 0017 RF4.1); without a terminal that
//! is a local error. Exit code: 0 when the reading is ready, 2 when the
//! run failed or broke the contract, 1 for local errors (unknown id, no
//! data directory, declined approval) — the pattern of the other commands.

use std::io::{self, BufRead, IsTerminal, Write};

use cuotai_core::extensions::consent::{self, Approved, Consent};
use cuotai_core::extensions::runner::{Outcome, Reading, Runner};
use cuotai_core::extensions::{self, Manifest};
use cuotai_core::text::Localized;
use cuotai_core::Status;

use crate::ui;

/// The `--extension-check <id>` command. See the module docs for the flow
/// and the exit codes.
pub(crate) async fn run(id: &str, json: bool) -> Result<u8, Box<dyn std::error::Error>> {
    let locale = ui::tui_locale();
    let Some(dir) = extensions::default_dir() else {
        return Err(no_dir().resolve(locale).into());
    };
    let discovery = extensions::discover(&dir);
    let Some(manifest) = discovery.get(id).cloned() else {
        return Err(unknown(id).resolve(locale).into());
    };
    let consent = Consent::in_data_dir().map_err(|error| error.resolve(locale))?;
    let interactive = io::stdin().is_terminal() && io::stderr().is_terminal();
    let approved = confirm(
        &consent,
        manifest,
        interactive,
        &mut io::stdin().lock(),
        &mut io::stderr().lock(),
        locale,
    )?;
    let reading = Runner::new().run(&approved).await;
    let mut stdout = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(&mut stdout, &reading_json(&reading))?;
        writeln!(stdout)?;
    } else {
        write!(stdout, "{}", reading_text(&reading, locale))?;
    }
    Ok(if reading.checked.status == Status::Ready {
        0
    } else {
        2
    })
}

/// `manifest` approved exactly as it is. Unapproved (never, or since a
/// change) the disclosure is written to `out` and the typed answer decides
/// in a terminal; otherwise a local error and nothing runs.
fn confirm(
    consent: &Consent,
    manifest: Manifest,
    interactive: bool,
    input: &mut impl BufRead,
    out: &mut impl Write,
    locale: &str,
) -> Result<Approved, String> {
    let io_error = |error: io::Error| error.to_string();
    let manifest = match consent.check(manifest) {
        Ok(approved) => return Ok(approved),
        Err(manifest) => *manifest,
    };
    for line in consent::disclosure(&manifest) {
        writeln!(out, "{}", line.resolve(locale)).map_err(io_error)?;
    }
    if !interactive {
        return Err(consent::not_approved(&manifest.id).resolve(locale));
    }
    let answer = consent::confirm_answer().resolve(locale);
    write!(
        out,
        "{} ",
        consent::confirm_question(&answer).resolve(locale)
    )
    .map_err(io_error)?;
    out.flush().map_err(io_error)?;
    let mut typed = String::new();
    input.read_line(&mut typed).map_err(io_error)?;
    if typed.trim() != answer {
        return Err(consent::declined().resolve(locale));
    }
    consent
        .approve(&manifest)
        .map_err(|error| error.resolve(locale))
}

/// The `--extension-check` JSON: how the run ended and the checked
/// reading, never the raw output.
fn reading_json(reading: &Reading) -> serde_json::Value {
    serde_json::json!({
        "id": &reading.run.id,
        "outcome": reading.run.outcome,
        "exit_code": reading.run.exit_code,
        "duration_ms": u64::try_from(reading.run.duration.as_millis()).unwrap_or(u64::MAX),
        "truncated": reading.run.truncated,
        "checked": &reading.checked,
    })
}

/// The `--extension-check` text report: the run's outcome, the normalized
/// reading (status, metrics, notes) and the contract violations, never
/// the raw output.
fn reading_text(reading: &Reading, locale: &str) -> String {
    let mut out = String::new();
    let ms = u64::try_from(reading.run.duration.as_millis()).unwrap_or(u64::MAX);
    let outcome = match (reading.run.outcome, reading.run.exit_code) {
        (Outcome::Exited, Some(code)) => format!("código {code}"),
        (Outcome::Exited, None) => "sem código de saída".to_string(),
        (Outcome::Terminated, _) => "encerrada sem código de saída".to_string(),
        (Outcome::TimedOut, _) => "tempo esgotado".to_string(),
        (Outcome::NotStarted, _) => "programa não iniciado".to_string(),
        (Outcome::Changed, _) => "mudou desde a aprovação".to_string(),
    };
    out += &format!("Execução: {outcome} em {ms} ms\n");
    out += &format!("Fonte: {}\n", ui::safe(&reading.checked.source));
    let status = cuotai_core::text::status_l10n(reading.checked.status).resolve(locale);
    out += &format!("Situação: {status}\n");
    for metric in &reading.checked.metrics {
        out += &format!(
            "  {}: {}\n",
            ui::safe(&metric.label),
            ui::metric_text(metric)
        );
    }
    for note in &reading.checked.notes {
        out += &format!("  {}\n", ui::safe(&note.resolve(locale)));
    }
    if !reading.checked.violations.is_empty() {
        out += "Violações do contrato:\n";
        for violation in &reading.checked.violations {
            out += &format!("  - {}\n", ui::safe(&violation.resolve(locale)));
        }
    }
    out
}

/// `extensions.check.no_dir`.
fn no_dir() -> Localized {
    Localized::fixed(
        "extensions.check.no_dir",
        "O diretório de dados local está indisponível; não é possível descobrir extensões.",
    )
}

/// `extensions.check.unknown`: no valid extension has this id.
fn unknown(id: &str) -> Localized {
    Localized::with_param(
        "extensions.check.unknown",
        "id",
        id,
        format!("Não há nenhuma extensão válida com o id {id}."),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A sandbox with one extension and its consent file.
    struct Rig {
        dir: std::path::PathBuf,
    }

    impl Rig {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!(
                "cuotai-tui-extension-check-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let extension = dir.join("proxy");
            fs::create_dir_all(&extension).unwrap();
            fs::write(
                extension.join("manifest.toml"),
                "id = \"proxy\"\nname = \"Proxy balance\"\nversion = \"1.2.0\"\nschema = 1\n\
                 command = [\"./run.sh\"]\ninterval_seconds = 300\n\
                 env = [\"PROXY_TOKEN\"]\n",
            )
            .unwrap();
            let program = extension.join("run.sh");
            fs::write(&program, "#!/bin/sh\nprintf '{}'\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
            }
            Self { dir }
        }

        fn manifest(&self) -> Manifest {
            let discovery = extensions::discover(&self.dir);
            assert!(discovery.rejected.is_empty(), "{:?}", discovery.rejected);
            discovery.get("proxy").expect("discovered").clone()
        }

        fn consent(&self) -> Consent {
            Consent::at(self.dir.join("consent.json"))
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    /// RF4.1: unapproved, the extension is disclosed (exact command,
    /// directory, requested variables, interval) and runs only after the
    /// typed answer; a change asks again.
    #[test]
    fn an_extension_runs_only_after_the_typed_approval() {
        let rig = Rig::new();
        let consent = rig.consent();
        let ask = |interactive, typed: &str| {
            let mut out = Vec::new();
            let result = confirm(
                &consent,
                rig.manifest(),
                interactive,
                &mut typed.as_bytes(),
                &mut out,
                "en-US",
            );
            (result, String::from_utf8(out).unwrap())
        };

        let (result, out) = ask(false, "");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.contains("not approved yet"), "{error}");
        assert!(out.contains("command: [\"./run.sh\"]"), "{out}");
        let dir = rig.dir.join("proxy").canonicalize().unwrap();
        assert!(out.contains(&dir.display().to_string()), "{out}");
        assert!(out.contains("PROXY_TOKEN"), "{out}");
        assert!(out.contains("300 s"), "{out}");
        assert!(out.contains("review the code"), "{out}");
        assert!(
            !out.contains("type"),
            "no question without a terminal: {out}"
        );

        let (result, out) = ask(true, "no\n");
        assert_eq!(
            result.unwrap_err(),
            "Extension not approved; it was not run."
        );
        assert!(out.ends_with("(any other answer declines): "), "{out}");
        assert!(consent.check(rig.manifest()).is_err(), "still unapproved");

        let (result, _) = ask(true, "yes\n");
        assert!(result.is_ok(), "{result:?}");
        let (result, out) = ask(false, "");
        assert!(result.is_ok(), "an approved extension needs no terminal");
        assert_eq!(out, "", "and is not disclosed again");

        // A change to the program asks again.
        fs::write(
            rig.dir.join("proxy").join("run.sh"),
            "#!/bin/sh\nprintf '{\"schema\":1}'\n",
        )
        .unwrap();
        let (result, out) = ask(false, "");
        assert!(result.is_err(), "a changed program needs a new approval");
        assert!(out.contains("command:"), "{out}");
    }

    /// RF2.4: the text report shows the run, the normalized reading and the
    /// violations; the JSON carries the same; the raw output never appears.
    #[test]
    fn the_report_is_normalized_and_never_raw() {
        use chrono::Utc;
        use cuotai_core::extensions::output;
        use cuotai_core::extensions::runner::Run;
        use std::time::Duration;

        let reading = |stdout: &[u8], code: i32| Reading {
            run: Run {
                id: "proxy".to_string(),
                started_at: Utc::now(),
                duration: Duration::from_millis(12),
                outcome: Outcome::Exited,
                exit_code: Some(code),
                stdout: stdout.to_vec(),
                truncated: false,
            },
            checked: output::check("proxy", stdout),
        };
        let ready = reading(
            br#"{"schema":1,"status":"ready","metrics":[{"label":"Saldo","unit":"cr","kind":"quota","used":1,"limit":2}],"notes":[]}"#,
            0,
        );
        assert_eq!(ready.checked.status, Status::Ready);
        let text = reading_text(&ready, "en-US");
        assert!(text.contains("código 0 em 12 ms"), "{text}");
        assert!(text.contains("Fonte: extension:proxy"), "{text}");
        assert!(text.contains("Saldo"), "{text}");
        assert!(!text.contains("Violações"), "{text}");
        let json = reading_json(&ready);
        assert_eq!(json["id"], "proxy");
        assert_eq!(json["outcome"], "exited");
        assert_eq!(json["exit_code"], 0);
        assert_eq!(json["duration_ms"], 12);
        assert_eq!(json["checked"]["status"], "ready");
        assert_eq!(json["checked"]["metrics"][0]["label"], "Saldo");

        // A broken contract: the violations name fields; the raw output,
        // even a distinctive value, is nowhere.
        let broken = reading(
            br#"{"schema":1,"status":"ready","metrics":[{"label":"RAW-MARKER","unit":"cr","kind":"nope"}],"notes":[]}"#,
            0,
        );
        assert_eq!(broken.checked.status, Status::Error);
        let text = reading_text(&broken, "en-US");
        assert!(text.contains("Violações do contrato:"), "{text}");
        assert!(text.contains("metrics[0].kind"), "{text}");
        assert!(!text.contains("RAW-MARKER"), "{text}");
        let json = serde_json::to_string(&reading_json(&broken)).unwrap();
        assert!(json.contains("metrics[0].kind"), "{json}");
        assert!(!json.contains("RAW-MARKER"), "{json}");
    }
}
