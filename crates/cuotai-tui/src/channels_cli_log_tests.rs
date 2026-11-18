//! PRD 0044 re-validation (V-04, V-08, V-14): `--channel-log` and the
//! consent title read in the TUI locale, with aligned columns.

use super::*;
use crate::l10n::with_locale;
use chrono::TimeDelta;
use unicode_width::UnicodeWidthStr;

fn entry(at: chrono::DateTime<Utc>, kind: &str, outcome: LogOutcome, status: Option<u16>) -> Entry {
    Entry {
        at,
        channel: "ch_1".into(),
        kind: kind.into(),
        outcome,
        status,
        attempt: 1,
        error: None,
    }
}

fn written(entries: &[Entry]) -> String {
    let mut out = Vec::new();
    write_log(entries, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn the_log_reads_in_the_tui_locale() {
    let now = Utc::now();
    let mut failed = entry(now, "threshold", LogOutcome::Failed, None);
    failed.error = Some("network".into());
    let entries = [
        entry(now, "threshold", LogOutcome::Delivered, Some(200)),
        failed,
        entry(now, "rate_limit", LogOutcome::RateLimited, None),
    ];
    let text = with_locale("de", || written(&entries));
    for raw in [
        "threshold",
        "delivered",
        "network",
        "rate_limit",
        "rate_limited",
    ] {
        assert!(!text.contains(raw), "{raw} in {text}");
    }
    let threshold = channels::NoticeKind::Threshold.label().resolve("de");
    assert!(text.contains(&threshold), "{text}");
    assert!(
        text.contains(&log::error_label("network").resolve("de")),
        "{text}"
    );
}

#[test]
fn the_status_column_lines_up_for_older_entries() {
    let now = Utc::now();
    let entries = [
        entry(
            now - TimeDelta::days(3),
            "test",
            LogOutcome::Delivered,
            Some(201),
        ),
        entry(now, "test", LogOutcome::Failed, Some(503)),
    ];
    let text = with_locale("en-US", || written(&entries));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    let column = |line: &str, status: &str| line[..line.find(status).unwrap()].width();
    assert_eq!(column(lines[0], "201"), column(lines[1], "503"), "{text}");
}

#[test]
fn a_webhook_consent_title_is_lowercase() {
    struct Nothing;
    impl cuotai_core::oauth::TokenStore for Nothing {
        fn load(
            &self,
            _: &str,
        ) -> Result<Option<cuotai_core::oauth::OAuthToken>, cuotai_core::oauth::OAuthError>
        {
            Ok(None)
        }
        fn save(
            &self,
            _: &str,
            _: &cuotai_core::oauth::OAuthToken,
        ) -> Result<(), cuotai_core::oauth::OAuthError> {
            Ok(())
        }
        fn delete(&self, _: &str) -> Result<(), cuotai_core::oauth::OAuthError> {
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let args = AddArgs {
        kind: "webhook",
        label: Some("Ops"),
        filters: &[],
        sign: false,
        yes: false,
    };
    let mut err = Vec::new();
    // Piped without --yes: the consent is printed and nothing is saved.
    let result = with_locale("en-US", || {
        add_with(
            &dir.path().join("desktop.json"),
            &Vault::new(Arc::new(Nothing)),
            &args,
            &mut io::Cursor::new(b"https://example.com/hook\n".as_slice()),
            &mut read_line,
            false,
            &mut Vec::new(),
            &mut err,
        )
    });
    assert!(result.is_err());
    let err = String::from_utf8(err).unwrap();
    assert!(err.contains("New webhook channel \"Ops\""), "{err}");
}
