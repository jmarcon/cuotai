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

use cuotai_core::extensions;
use cuotai_core::extensions::consent;
use cuotai_core::extensions::management::Manager;
use cuotai_core::extensions::runner::{Outcome, Reading};
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
    let manager = Manager::at(dir);
    if !manager
        .is_enabled(id)
        .map_err(|error| error.resolve(locale))?
    {
        // RF5.3: the approval is bound to exactly what these lines show.
        let disclosure = manager
            .disclosure(id)
            .map_err(|error| error.resolve(locale))?;
        for line in &disclosure.lines {
            let text = line.resolve(locale);
            writeln!(io::stderr().lock(), "{}", ui::safe(&text))?;
        }
        let interactive = io::stdin().is_terminal() && io::stderr().is_terminal();
        if !interactive {
            return Err(consent::not_approved(id).resolve(locale).into());
        }
        let answer = consent::confirm_answer().resolve(locale);
        let mut stderr = io::stderr().lock();
        write!(
            stderr,
            "{} ",
            consent::confirm_question(&answer).resolve(locale)
        )?;
        stderr.flush()?;
        let mut typed = String::new();
        io::stdin().lock().read_line(&mut typed)?;
        manager
            .approve(&disclosure.candidate, typed.trim() == answer)
            .map_err(|error| error.resolve(locale))?;
    }
    let reading = manager
        .test_reading(id)
        .await
        .map_err(|error| error.resolve(locale))?;
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
        (Outcome::Revoked, _) => "aprovação revogada".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use cuotai_core::Config;
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
            // A real executable compiled here, so the fixture runs on every
            // platform with no shell and no interpreter in the contract.
            let name = if cfg!(windows) {
                "program.exe"
            } else {
                "run.sh"
            };
            let source = extension.join("program.rs");
            let source_text = format!(
                "fn main() {{ print!(\"{{}}\", {output:?}); }}",
                output = r#"{"schema":1,"status":"ready","metrics":[{"label":"Saldo","unit":"cr","kind":"quota","used":1,"limit":2,"remaining":1}],"notes":[]}"#,
            );
            fs::write(&source, source_text).unwrap();
            let program = extension.join(name);
            let built = std::process::Command::new("rustc")
                .args(["--edition", "2021", "-O"])
                .arg(&source)
                .arg("-o")
                .arg(&program)
                .output()
                .expect("rustc builds the fixture program");
            assert!(
                built.status.success(),
                "fixture build failed: {}",
                String::from_utf8_lossy(&built.stderr)
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
            }
            fs::write(
                extension.join("manifest.toml"),
                format!(
                    "id = \"proxy\"\nname = \"Proxy balance\"\nversion = \"1.2.0\"\nschema = 1\n\
                     command = [\"./{name}\"]\ninterval_seconds = 300\n\
                     env = [\"PROXY_TOKEN\"]\n"
                ),
            )
            .unwrap();
            Self { dir }
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    /// RF5.3: the approval binds to exactly the disclosed files. A program
    /// that changes between the disclosure and the confirmation fails closed
    /// (no approval, no run), and a token cannot be replayed.
    #[test]
    fn an_approval_binds_to_the_disclosed_files_and_is_single_use() {
        let rig = Rig::new();
        let manager = Manager::at(&rig.dir);

        // Nothing is approved before a confirmation.
        assert!(!manager.is_enabled("proxy").unwrap());
        let disclosure = manager.disclosure("proxy").unwrap();
        let shown = disclosure
            .lines
            .iter()
            .map(|line| line.resolve("en-US"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(shown.contains("command:"), "{shown}");
        assert!(
            shown.contains(
                &rig.dir
                    .join("proxy")
                    .canonicalize()
                    .unwrap()
                    .display()
                    .to_string()
            ),
            "{shown}"
        );
        assert!(shown.contains("PROXY_TOKEN"), "{shown}");
        assert!(shown.contains("300 s"), "{shown}");
        assert!(shown.contains("review the code"), "{shown}");

        // The program changes while the user reads: the same confirmation
        // must not approve what is now on disk.
        let changed = rig.dir.join("proxy").join("program.rs");
        fs::write(
            &changed,
            r#"fn main() { std::io::Write::write_all(&mut std::io::stdout(), b"changed").unwrap(); }"#,
        )
        .unwrap();
        let rebuilt = std::process::Command::new("rustc")
            .args(["--edition", "2021", "-O"])
            .arg(&changed)
            .arg("-o")
            .arg(rig.dir.join("proxy").join(if cfg!(windows) {
                "program.exe"
            } else {
                "run.sh"
            }))
            .output()
            .unwrap();
        assert!(rebuilt.status.success());
        let error = manager
            .enable(&disclosure.candidate, true, &Config::default())
            .unwrap_err();
        assert_eq!(error.id, "extensions.action.changed", "{error:?}");
        assert!(!manager.is_enabled("proxy").unwrap());

        // A fresh disclosure of the new files approves, and its token is
        // spent: replaying it approves nothing.
        let fresh = manager.disclosure("proxy").unwrap();
        let enabled = manager
            .enable(&fresh.candidate, true, &Config::default())
            .unwrap();
        assert!(enabled.config.validate().is_ok(), "{:?}", enabled.config);
        assert!(manager.is_enabled("proxy").unwrap());
        let replay = manager
            .enable(&fresh.candidate, true, &enabled.config)
            .unwrap_err();
        assert_eq!(
            replay.id, "extensions.action.candidate_expired",
            "{replay:?}"
        );

        // Disabling revokes it again.
        manager.disable("proxy").unwrap();
        assert!(!manager.is_enabled("proxy").unwrap());
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
