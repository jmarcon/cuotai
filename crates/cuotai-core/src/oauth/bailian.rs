//! Alibaba Token Plan (Qwen Cloud) login via the provider's own `bl` CLI.
//!
//! Runs `bl auth login --console --console-site <site> --config <profile>` in
//! an isolated per-profile subprocess. The CLI writes to its own local vault;
//! Cuotai stores nothing and promises no rollback (RF2.4): cancel after the
//! CLI finished leaves whatever `bl` persisted.
//!
//! Security invariants (unchanged from the previous TUI implementation):
//! - stdout/stderr are drained to the void and never inspected — the CLI may
//!   print keys, cookies and other sensitive metadata;
//! - argv is a fixed list plus validated values; no env, no shell, no stdin;
//! - `kill_on_drop(true)` + timeout guarantee cancellation kills the process;
//! - API keys, quota cookies and OAuth credentials NEVER appear in argv.

use super::OAuthError;

/// Maximum time for `bl auth login --console`. `bl` may open a browser and
/// wait for human interaction; 10 minutes is enough slack without holding the UI.
const BAILIAN_LOGIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Sites supported by `bl --console --console-site` in the international Token Plan.
/// Refuses any value outside this list so as never to pass arbitrary
/// arguments to the subprocess.
const BAILIAN_ALLOWED_SITES: &[&str] = &["international"];

/// Validates the `--console-site` argument before building argv. Unknown
/// sites become `Configuration` with the allow-list shown to the user.
fn validate_console_site(site: &str) -> Result<(), OAuthError> {
    let trimmed = site.trim();
    if trimmed.is_empty() {
        return Err(OAuthError::bailian_console_site_blank());
    }
    if !BAILIAN_ALLOWED_SITES.contains(&trimmed) {
        return Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.bailian.unknown_console_site",
            "Unknown console site. Accepted values: international.",
        )));
    }
    if trimmed.starts_with('-') || trimmed.chars().any(char::is_control) {
        return Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.bailian.invalid_console_site",
            "Invalid console site.",
        )));
    }
    Ok(())
}

/// Validates the dedicated profile name (`bl`'s `--config <name>`). Limits it to
/// safe ASCII so the token is never interpreted as a flag and never
/// collides with the CLI's `argv`: letters, digits, hyphen and underscore; no
/// leading `-`; up to 32 characters. The profile is ALWAYS passed as a separate
/// argv argument — never concatenated.
fn validate_cli_profile(name: &str) -> Result<(), OAuthError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.bailian.profile_blank",
            "The `bl` profile is blank; cannot start the isolated login.",
        )));
    }
    if trimmed.len() > 32 {
        return Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.bailian.profile_too_long",
            "The `bl` profile exceeds 32 characters.",
        )));
    }
    if trimmed.starts_with('-') {
        return Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.bailian.profile_leading_hyphen",
            "The `bl` profile cannot start with '-'.",
        )));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.bailian.profile_invalid_chars",
            "The `bl` profile accepts only letters, digits, `-` and `_`.",
        )));
    }
    Ok(())
}

/// Spawn do `bl auth login --console --console-site <site> --config <profile>`.
///
/// Dropping the returned future kills the child (`kill_on_drop`).
pub(crate) async fn console_login(console_site: &str, profile: &str) -> Result<(), OAuthError> {
    use std::process::Stdio;
    use tokio::process::Command;

    validate_console_site(console_site)?;
    validate_cli_profile(profile)?;

    // argv is a fixed list: `auth`, `login`, `--console`, `--console-site`,
    // `<site>`, `--config`, `<profile>` — each item separate, no concatenation,
    // no shell. No API key, cookie or token.
    let mut child = Command::new("bl")
        .args([
            "auth",
            "login",
            "--console",
            "--console-site",
            console_site,
            "--config",
            profile,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                OAuthError::Configuration(crate::text::Localized::fixed(
                    "oauth_error.bailian.cli_not_found",
                    "`bl` (Bailian CLI) not found. Install the CLI and try again.",
                ))
            }
            _ => OAuthError::Configuration(crate::text::Localized::fixed(
                "oauth_error.bailian.cli_start_failed",
                "Could not start `bl auth login --console`.",
            )),
        })?;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    let timed = tokio::time::timeout(BAILIAN_LOGIN_TIMEOUT, async {
        let (status, (), ()) =
            tokio::join!(child.wait(), drain_silent(stdout), drain_silent(stderr),);
        status
    })
    .await;

    match timed {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(_)) => Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.bailian.console_login_failed",
            "`bl auth login --console` exited with an error. Check the CLI and try again.",
        ))),
        Ok(Err(_)) => Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.bailian.cli_wait_failed",
            "Failed while waiting for `bl`.",
        ))),
        Err(_) => {
            // Timeout: kills the child before returning an error.
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(OAuthError::Expired)
        }
    }
}

/// Silently discards any stream content — nothing ever reaches
/// the UI or logs. Keeps the pipe active so the child never blocks.
async fn drain_silent(mut stream: impl tokio::io::AsyncRead + Unpin) {
    let _ = tokio::io::copy(&mut stream, &mut tokio::io::sink()).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_site_accepts_only_known_values() {
        // `international` is the only site currently accepted.
        assert!(validate_console_site("international").is_ok());
        // Does not pass arbitrary argv to the subprocess.
        for bad in [
            "",
            "   ",
            "-rf",
            "--help",
            "cn",
            "china",
            "mainland",
            "with spaces",
            "internat\x00ional",
        ] {
            let err = validate_console_site(bad)
                .err()
                .unwrap_or_else(|| panic!("esperava rejeição para {bad:?}"));
            assert!(
                matches!(err, OAuthError::Configuration(_)),
                "rejeição inesperada para {bad:?}: {err:?}"
            );
        }
    }

    #[test]
    fn cli_profile_rejects_argv_unsafe_names() {
        assert!(validate_cli_profile("cuotai-test").is_ok());
        assert!(validate_cli_profile("qwen-personal").is_ok());
        // Profiles with a leading '-' or non-ASCII-safe characters are
        // rejected before spawn so they never collide with argv parsing.
        for bad in [
            "-leading-dash",
            "with space",
            "uni\u{0301}code",
            "way-too-long-profile-name-that-exceeds-the-limit",
            "",
            "   ",
        ] {
            let err = validate_cli_profile(bad)
                .err()
                .unwrap_or_else(|| panic!("esperava rejeição para {bad:?}"));
            assert!(
                matches!(err, OAuthError::Configuration(_)),
                "rejeição inesperada para {bad:?}: {err:?}"
            );
        }
    }
}
