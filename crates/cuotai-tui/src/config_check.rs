//! PRD 0051 RF2.3/RF2.4: `cuotai --check-config [--json]`. The file is the
//! one the other modes would read (`--config`, `CUOTAI_CONFIG`, an existing
//! `./cuotai.toml`, then the OS path); the check is the core's strict,
//! offline one. Exit 0 when valid, 1 otherwise.

use cuotai_core::config_file::{self, ConfigCheck, ConfigFile, Discovery, Locations};
use std::io::{self, Write};
use std::path::PathBuf;

use crate::l10n;

/// Run the check and print it; returns the exit code.
pub(crate) fn run(config: Option<PathBuf>, json: bool, locale: &str) -> io::Result<u8> {
    let resolved = ConfigFile::resolve_path(Discovery::Cli { config }, &Locations::from_process());
    let check = config_file::check(&resolved.path);
    let mut stdout = io::stdout().lock();
    write!(
        stdout,
        "{}",
        render(&check, &resolved.path.display().to_string(), json, locale)
    )?;
    Ok(if check.valid { 0 } else { 1 })
}

fn render(check: &ConfigCheck, path: &str, json: bool, locale: &str) -> String {
    if json {
        return serde_json::to_string_pretty(check).unwrap_or_default() + "\n";
    }
    if check.valid {
        return l10n::trf_in(
            locale,
            "cli.check_config.valid",
            "The configuration {path} is valid.",
            &[("path", &path)],
        ) + "\n";
    }
    check
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.note.resolve(locale) + "\n")
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_and_json_output_name_the_path_but_never_the_value() {
        let check = config_file::check_bytes(
            b"[[accounts]]\nprovider = \"groq\"\ncredential_env = \"K\"\nnote = \"sk-live-123\"\n",
        );
        let human = render(&check, "/x/cuotai.toml", false, "en-US");
        assert!(human.contains("accounts[0].note"), "{human}");
        assert!(human.contains("line 4"), "{human}");
        assert!(!human.contains("sk-live"), "{human}");
        let json: serde_json::Value =
            serde_json::from_str(&render(&check, "/x/cuotai.toml", true, "en-US")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "valid": false,
                "diagnostics": [{ "kind": "parse", "line": 4, "column": 1, "path": "accounts[0].note" }]
            })
        );
        let valid = config_file::check_bytes(b"accounts = []\n");
        assert_eq!(
            render(&valid, "/x/cuotai.toml", false, "en-US"),
            "The configuration /x/cuotai.toml is valid.\n"
        );
    }
}
