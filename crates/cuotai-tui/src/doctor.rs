//! PRD 0046 RF4: `cuotai --doctor [--json]`, the core's read-only and
//! offline report of this installation, grouped by status with each fix.
//! Exit 0 without a failure, 2 with one, 1 when the doctor itself fails.

use cuotai_core::doctor::{self, CheckStatus, Inputs, Report};
use std::io::{self, Write};
use std::path::PathBuf;

use crate::l10n;

pub(crate) fn run(
    config: Option<PathBuf>,
    install_source: String,
    json: bool,
    locale: &str,
) -> io::Result<u8> {
    let report = doctor::run(&Inputs::from_process(config, install_source));
    let mut stdout = io::stdout().lock();
    write!(stdout, "{}", render(&report, json, locale))?;
    Ok(report.exit_code())
}

fn heading(status: CheckStatus, locale: &str) -> &'static str {
    match status {
        CheckStatus::Fail => l10n::tr_in(locale, "cli.doctor.fail", "Needs attention"),
        CheckStatus::Warn => l10n::tr_in(locale, "cli.doctor.warn", "Worth a look"),
        CheckStatus::Ok => l10n::tr_in(locale, "cli.doctor.ok", "Fine"),
        CheckStatus::Skip => l10n::tr_in(locale, "cli.doctor.skip", "Not checked"),
    }
}

fn mark(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Fail => "✗",
        CheckStatus::Warn => "!",
        CheckStatus::Ok => "✓",
        CheckStatus::Skip => "·",
    }
}

pub(crate) fn render(report: &Report, json: bool, locale: &str) -> String {
    if json {
        return serde_json::to_string_pretty(report).unwrap_or_default() + "\n";
    }
    let mut out = String::new();
    for status in [
        CheckStatus::Fail,
        CheckStatus::Warn,
        CheckStatus::Ok,
        CheckStatus::Skip,
    ] {
        let checks: Vec<_> = report
            .checks
            .iter()
            .filter(|check| check.status == status)
            .collect();
        if checks.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(heading(status, locale));
        out.push('\n');
        for check in checks {
            out.push_str(&format!(
                "  {} {}",
                mark(status),
                check.title.resolve(locale)
            ));
            if let Some(detail) = &check.detail {
                out.push_str(&format!(": {}", detail.resolve(locale)));
            }
            out.push('\n');
            if let Some(fix) = &check.fix {
                out.push_str(&format!("    → {}\n", fix.resolve(locale)));
            }
        }
    }
    // PRD 0054 RF6.3: one fixed `tool.<name>: …` line per provider CLI.
    if !report.tools.is_empty() {
        out.push('\n');
        for tool in report.tools.values() {
            out.push_str(&tool.line());
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuotai_core::doctor::Check;
    use cuotai_core::text::Localized;

    #[test]
    fn checks_are_grouped_by_status_with_their_fix() {
        let check = |id: &str, status, fix: Option<&str>| Check {
            id: id.to_owned(),
            status,
            title: Localized::fixed("x", id.to_owned()),
            detail: None,
            fix: fix.map(|text| Localized::fixed("y", text.to_owned())),
        };
        let report = Report {
            schema: 1,
            checks: vec![
                check("first", CheckStatus::Ok, None),
                check("second", CheckStatus::Fail, Some("do this")),
            ],
            tools: Default::default(),
        };
        assert_eq!(
            render(&report, false, "en-US"),
            "Needs attention\n  ✗ second\n    → do this\n\nFine\n  ✓ first\n"
        );
    }
}
