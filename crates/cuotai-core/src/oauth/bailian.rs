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

/// Tempo máximo para `bl auth login --console`. O `bl` pode abrir navegador e
/// esperar interação humana; 10 minutos é folga suficiente sem prender a UI.
const BAILIAN_LOGIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Sites suportados pelo `bl --console --console-site` no Token Plan internacional.
/// Recusa qualquer valor fora desta lista para nunca repassar argumentos
/// arbitrários ao subprocesso.
const BAILIAN_ALLOWED_SITES: &[&str] = &["international"];

/// Valida o argumento `--console-site` antes de montar a argv. Sites não
/// conhecidos viram `Configuration` com a lista branca exibida ao usuário.
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

/// Valida o nome do perfil dedicado (`--config <name>` do `bl`). Limita a
/// ASCII seguro para que o token nunca seja interpretado como flag e nunca
/// colida com `argv` do CLI: letras, dígitos, hífen e underscore; sem
/// leading `-`; até 32 caracteres. O perfil é SEMPRE passado como um
/// argumento argv separado — nunca concatenado.
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

    // argv é uma lista fixa: `auth`, `login`, `--console`, `--console-site`,
    // `<site>`, `--config`, `<profile>` — cada item separado, sem concatenação,
    // sem shell. Nada de API key, cookie, token.
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
            // Tempo esgotado: mata o filho antes de devolver erro.
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(OAuthError::Expired)
        }
    }
}

/// Descarta silenciosamente qualquer conteúdo do stream — nada jamais chega
/// à UI nem a logs. Mantém o pipe ativo para o filho nunca bloquear.
async fn drain_silent(mut stream: impl tokio::io::AsyncRead + Unpin) {
    let _ = tokio::io::copy(&mut stream, &mut tokio::io::sink()).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_site_accepts_only_known_values() {
        // `international` é o único site atualmente aceito.
        assert!(validate_console_site("international").is_ok());
        // Não repassa argv arbitrária ao subprocesso.
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
        // Perfis com leading '-' ou caracteres não-ASCII-seguros são
        // rejeitados antes do spawn para nunca colidir com parsing de argv.
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
