//! AWS IAM Identity Center (SSO) login delegated to the official AWS CLI.
//!
//! Runs `aws sso login --profile P --use-device-code --no-browser`; the CLI
//! performs the device authorization and manages the SSO token cache itself.
//! Cuotai never reads AWS config, credentials or cache files and stores no
//! AWS token: exit status 0 means the CLI completed authentication.
//! Reference: https://docs.aws.amazon.com/cli/latest/reference/sso/login.html

use super::OAuthError;
use std::io::ErrorKind;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::mpsc::Sender;

/// Cap of stdout bytes inspected for auth lines; the CLI prompt is small.
const MAX_INSPECT_BYTES: usize = 64 * 1024;
/// Single forwarded line cap; drops stack traces or dumped blobs.
const MAX_LINE_LEN: usize = 512;
/// Whole device sign-in budget; the CLI's own device code expires sooner.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Starts the official AWS CLI SSO device login for a preconfigured profile
/// (`aws configure sso`), streaming only the verification URL and user code
/// through `events`. Dropping this future kills the CLI (kill_on_drop) and
/// drops the pipe readers; no detached tasks survive. Success means the CLI
/// finished authentication.
pub async fn login(profile: &str, events: Sender<String>) -> Result<(), OAuthError> {
    validate_profile(profile)?;

    let mut child = Command::new("aws")
        // Fixed argument vector; profile is an argv value, never a shell string.
        .args(["sso", "login", "--profile", profile, "--use-device-code", "--no-browser"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("AWS_PAGER", "")
        .env("AWS_CLI_AUTO_PROMPT", "off")
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.aws.cli_not_found", "AWS CLI (aws) não encontrada. Instale a AWS CLI v2 e configure o perfil com `aws configure sso`.",
        )),
            _ => OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.aws.cli_start_failed", "Não foi possível iniciar a AWS CLI.",
        )),
        })?;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    // Wait and consume both pipes concurrently in this task so the child never
    // blocks on a full pipe; stderr (arbitrary diagnostics) is discarded.
    let timed = tokio::time::timeout(LOGIN_TIMEOUT, async {
        let (status, (), ()) = tokio::join!(
            child.wait(),
            forward_auth_lines(stdout, events),
            drain(stderr),
        );
        status
    })
    .await;

    match timed {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(_)) => Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.aws.sso_login_incomplete", "Login SSO não concluído. Verifique a configuração SSO do perfil (`aws configure sso`) e tente novamente.",
        ))),
        Ok(Err(_)) => Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.aws.cli_wait_failed", "Falha ao aguardar a AWS CLI.",
        ))),
        Err(_) => {
            // Past the device sign-in budget: reap the child before returning.
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(OAuthError::Expired)
        }
    }
}

fn validate_profile(profile: &str) -> Result<(), OAuthError> {
    let ok = !profile.is_empty()
        && profile.len() <= 128
        && !profile.starts_with('-')
        && !profile.chars().any(char::is_control);
    if ok {
        Ok(())
    } else {
        Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.aws.invalid_profile_name",
            "Nome de perfil AWS inválido.",
        )))
    }
}

/// Discards a stream completely without closing it early, so the child can
/// always finish writing regardless of volume.
async fn drain(mut stream: impl AsyncRead + Unpin) {
    let _ = tokio::io::copy(&mut stream, &mut tokio::io::sink()).await;
}

/// Streams stdout, forwarding only the device authorization prompt lines
/// (verification URL, user code, short instructions). Only the first
/// `MAX_INSPECT_BYTES` are inspected; output beyond that is drained and
/// ignored so the pipe never closes early and arbitrary CLI output never
/// reaches the UI.
async fn forward_auth_lines(mut stdout: impl AsyncRead + Unpin, events: Sender<String>) {
    let mut inspected = 0usize;
    let mut pending = String::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stdout.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if inspected >= MAX_INSPECT_BYTES {
                    continue; // Drain only; never block the child.
                }
                let take = n.min(MAX_INSPECT_BYTES - inspected);
                inspected += take;
                pending.push_str(&String::from_utf8_lossy(&chunk[..take]));
                while let Some(pos) = pending.find('\n') {
                    let line: String = pending.drain(..=pos).collect();
                    forward_line(&line, &events).await;
                }
                if pending.len() > MAX_LINE_LEN * 2 {
                    pending.clear(); // Overlong partial line: drop it.
                }
            }
        }
    }
    if !pending.is_empty() {
        forward_line(&pending, &events).await;
    }
}

async fn forward_line(line: &str, events: &Sender<String>) {
    let line = sanitize(line);
    if line.is_empty() || line.len() > MAX_LINE_LEN {
        return;
    }
    let safe = match extract_sso_url(&line) {
        Some(url) => Some(url), // Forward the validated official URL.
        None if is_user_code(&line) || is_instruction(&line) => Some(line.clone()),
        None => None,
    };
    if let Some(msg) = safe {
        // Receiver gone means the wizard was cancelled; just keep draining.
        let _ = events.send(msg).await;
    }
}

/// Removes ANSI escapes/control characters, keeping plain display text.
fn sanitize(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // Skip CSI sequence: ESC '[' ... final byte in @..~
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            continue;
        }
        if !c.is_control() {
            out.push(c);
        }
    }
    out.trim().to_string()
}

/// Extracts the first https URL in the line and returns it only when it is a
/// genuine AWS IAM Identity Center device URL: https scheme, no credentials,
/// host exactly an AWS SSO/awsapps domain or a subdomain of one.
fn extract_sso_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let candidate: String = line[start..]
        .chars()
        .take_while(|c| !c.is_whitespace())
        .collect();
    let url = reqwest::Url::parse(&candidate).ok()?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let allowed = host == "awsapps.com"
        || host == "amazonaws.com"
        || host.ends_with(".awsapps.com")
        || host.ends_with(".amazonaws.com");
    allowed.then(|| url.to_string())
}

/// Known short instruction lines of the official device-flow prompt.
fn is_instruction(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("enter the code")
        || lower.contains("open the following url")
        || lower.contains("verification url")
        || lower.contains("authorization page")
}

/// AWS device user codes are two groups of four uppercase letters, e.g. `HRCW-KGJK`.
fn is_user_code(line: &str) -> bool {
    let parts: Vec<&str> = line.split('-').collect();
    parts.len() == 2
        && parts.iter().all(|p| {
            p.len() == 4
                && p.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_official_sso_urls() {
        for line in [
            "https://device.sso.us-east-1.amazonaws.com/",
            "https://device.sso.us-east-1.amazonaws.com/?user_code=HRCW-KGJK",
            "  https://d-1234567890.awsapps.com/start  ",
            "go to https://my-sso-portal.awsapps.com/start#/ now",
        ] {
            assert!(extract_sso_url(line).is_some(), "should accept: {line}");
        }
    }

    #[test]
    fn rejects_non_aws_or_deceptive_urls() {
        for line in [
            "https://sso.evil.example.com/",
            "https://evil-awsapps.com/",
            "https://awsapps.com.evil.example/",
            "https://user:secret@device.sso.us-east-1.amazonaws.com/",
            "http://device.sso.us-east-1.amazonaws.com/",
            "https://example.com/?next=sso.amazonaws.com",
            "not a url at all",
        ] {
            assert!(extract_sso_url(line).is_none(), "should reject: {line}");
        }
    }

    #[test]
    fn recognizes_device_user_codes() {
        assert!(is_user_code("HRCW-KGJK"));
        assert!(is_user_code("AB12-CD34"));
        assert!(!is_user_code("hrcw-kgjk"));
        assert!(!is_user_code("HRCWKGJK"));
        assert!(!is_user_code("HRCW-KGJ"));
        assert!(!is_user_code("HRCW-KGJK-XXXX"));
    }

    #[test]
    fn sanitize_strips_ansi_and_controls() {
        assert_eq!(sanitize("\u{1b}[32mOK\u{1b}[0m\r"), "OK");
        assert_eq!(sanitize("a\u{7}b"), "ab");
    }
}
