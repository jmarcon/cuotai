//! PRD 0028 RF2 — "Report a problem". A pure, secret-free issue draft for a
//! failing account: the facts a maintainer needs (version, platform,
//! provider, interface level, failure category, drifted paths) and nothing
//! else. Nothing is sent by Cuotai: the front ends show the text and let the
//! user open the prefilled GitHub URL or copy it.
//!
//! Never part of a draft: the account name, e-mail, organization, TOML
//! content, environment-variable names, notes, URLs, headers or tokens.

use crate::events::FailureCategory;
use crate::{access, Account, Report};
use std::time::Duration;

/// Repository the issue is opened on.
pub const REPOSITORY: &str = "https://github.com/jmarcon/cuotai";
/// The issue form the URL prefills.
pub const TEMPLATE: &str = "provider-problem.yml";
/// A longer URL is not offered as a link, only as text to copy (RF2.3).
pub const MAX_URL_CHARS: usize = 8_000;

/// The running application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppInfo {
    pub version: String,
    pub os: String,
    pub arch: String,
    /// [`crate::install_source::InstallSource::as_str`] or `unknown`.
    pub install_channel: String,
}

impl AppInfo {
    /// This build's version and platform with the given install channel.
    pub fn current(install_channel: &str) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            install_channel: install_channel.to_string(),
        }
    }
}

/// What is known of the failing account.
#[derive(Debug, Clone)]
pub struct AccountView<'a> {
    pub account: &'a Account,
    pub report: &'a Report,
    /// Age of the last good reading, when the account had one.
    pub last_good_age: Option<Duration>,
    /// Whether a provider incident was active (PRD 0011); `None` when status
    /// queries are off or the provider has no verified source.
    pub incident_active: Option<bool>,
}

/// The text and link of a problem report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueDraft {
    pub title: String,
    /// Markdown, in English (issue text is English per the repository rules).
    pub body: String,
    pub url: String,
    /// The URL is too long to open; only copying is offered.
    pub copy_only: bool,
}

/// Keep a value shown in a draft to plain, short, identifier-like text.
fn plain(value: &str) -> String {
    let kept: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
        .take(40)
        .collect();
    if kept.is_empty() {
        "unknown".into()
    } else {
        kept
    }
}

/// How the account authenticates, without naming any credential.
pub(crate) fn auth_mode(account: &Account) -> &'static str {
    if account.provider == crate::Provider::Extension {
        "extension"
    } else if account.oauth.is_some() {
        "native_login"
    } else if account.session.is_some() {
        "session_in_vault"
    } else {
        "environment_variable"
    }
}

fn age_text(age: Duration) -> String {
    let minutes = age.as_secs() / 60;
    match minutes {
        0..=59 => format!("{minutes} min ago"),
        60..=2879 => format!("{} h ago", minutes / 60),
        _ => format!("{} days ago", minutes / 1440),
    }
}

/// The problem report of `view`.
pub fn issue_draft(view: &AccountView<'_>, app: &AppInfo) -> IssueDraft {
    build(view, app, None)
}

/// PRD 0028 RF2.5: the draft the local canary offers for a drift: the same
/// secret-free facts plus the canary's `category` and its shape diff (paths
/// and types only, already redacted by the canary).
pub fn canary_draft(
    view: &AccountView<'_>,
    app: &AppInfo,
    category: &str,
    shape_diff: &str,
) -> IssueDraft {
    build(view, app, Some((category, shape_diff)))
}

/// Whether `url` opens this project's issue form: what a front end may hand
/// to the system browser for a problem report (never an arbitrary address).
pub fn is_issue_url(url: &str) -> bool {
    let prefix = format!("{REPOSITORY}/issues/new?");
    url.len() <= MAX_URL_CHARS * 4
        && url.starts_with(&prefix)
        && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// The draft of a problem that is not tied to an account (About): only the
/// application facts, with the issue form opened for the rest.
pub fn app_draft(app: &AppInfo) -> IssueDraft {
    let version = plain(&app.version);
    let os = plain(&app.os);
    let arch = plain(&app.arch);
    let channel = plain(&app.install_channel);
    let title = "[Provider] ".to_string();
    let body = format!(
        "## Problem report\n\n- Cuotai version: {version}\n- OS and architecture: {os} {arch}\n- Install channel: {channel}\n\nNo account name, e-mail, organization, credential, URL or response value is included.\n"
    );
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("template", TEMPLATE);
    query.append_pair("version", &version);
    query.append_pair("context", &body);
    let url = format!("{REPOSITORY}/issues/new?{}", query.finish());
    let copy_only = url.chars().count() > MAX_URL_CHARS;
    IssueDraft {
        title,
        body,
        url,
        copy_only,
    }
}

/// Keep a canary shape diff to plain, bounded single-line text.
fn plain_diff(diff: &str) -> String {
    diff.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(1_500)
        .collect()
}

fn build(view: &AccountView<'_>, app: &AppInfo, canary: Option<(&str, &str)>) -> IssueDraft {
    let account = view.account;
    let report = view.report;
    let slug = account.provider.slug();
    let access = access::describe(account);
    let category = canary.map_or_else(
        || {
            FailureCategory::of(report)
                .map_or("none", FailureCategory::name)
                .to_string()
        },
        |(category, _)| plain(category),
    );
    let category = category.as_str();
    let shape_diff = canary.map(|(_, diff)| plain_diff(diff));
    let mode = auth_mode(account);
    let level = serde_json::to_value(access.level)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into());
    let drift = report.diagnostic.as_ref().map(|d| d.drift());
    let paths = drift.map_or_else(String::new, |drift| {
        drift
            .paths
            .iter()
            .map(|p| format!("`{}` ({} -> {})", p.path, p.expected, p.found))
            .collect::<Vec<_>>()
            .join(", ")
    });
    let digest = drift.map(|d| d.digest.clone());
    let version = plain(&app.version);
    let os = plain(&app.os);
    let arch = plain(&app.arch);
    let channel = plain(&app.install_channel);

    let title = if canary.is_some() {
        format!("[Provider] {slug}: canary {category}")
    } else {
        format!("[Provider] {slug}: {category}")
    };
    let mut body = String::from("## Provider problem report\n\n");
    let mut line = |text: String| {
        body.push_str("- ");
        body.push_str(&text);
        body.push('\n');
    };
    line(format!("Cuotai version: {version}"));
    line(format!("OS and architecture: {os} {arch}"));
    line(format!("Install channel: {channel}"));
    line(format!("Provider: `{slug}`"));
    line(format!("Connection mode: `{mode}`"));
    line(format!(
        "Access level: {level} (verified on {})",
        access.verified_on
    ));
    line(format!("Failure category: `{category}`"));
    if !paths.is_empty() {
        line(format!("Drifted fields: {paths}"));
    }
    if let Some(diff) = &shape_diff {
        line(format!("Canary shape diff: {diff}"));
    }
    if let Some(digest) = &digest {
        line(format!("Response fingerprint (SHA-256): `{digest}`"));
    }
    line(format!(
        "Last good reading: {}",
        view.last_good_age
            .map_or_else(|| "none".to_string(), age_text)
    ));
    line(format!(
        "Provider incident active: {}",
        match view.incident_active {
            Some(true) => "yes",
            Some(false) => "no",
            None => "unknown",
        }
    ));
    if account.provider == crate::Provider::Extension {
        line("An extension reports this account.".to_string());
    } else if report.source.starts_with("extension:") {
        // PRD 0028 RF7.3: the built-in adapter did not answer.
        line(
            "An extension override is active: an extension replaced the built-in query."
                .to_string(),
        );
    }
    body.push_str(
        "\nNo account name, e-mail, organization, credential, URL or response value is included.\n",
    );

    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("template", TEMPLATE);
    query.append_pair("title", &title);
    query.append_pair("provider", slug);
    query.append_pair("version", &version);
    query.append_pair("mode", mode);
    query.append_pair("category", category);
    if !paths.is_empty() {
        query.append_pair("paths", &paths);
    }
    if let Some(diff) = &shape_diff {
        query.append_pair("shape_diff", diff);
    }
    query.append_pair("context", &body);
    let url = format!("{REPOSITORY}/issues/new?{}", query.finish());
    let copy_only = url.chars().count() > MAX_URL_CHARS;
    IssueDraft {
        title,
        body,
        url,
        copy_only,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic;
    use crate::{Provider, Status};
    use serde_json::json;

    fn report(account: &Account, status: Status) -> Report {
        Report {
            overage_hint: Default::default(),
            as_of: Default::default(),
            sub_reports: Vec::new(),
            account_id: None,
            account: account.name.clone(),
            provider: account.provider,
            checked_at: chrono::Utc::now(),
            status,
            metrics: Vec::new(),
            notes: vec!["token sk-live-SEEDED at https://api.example.com/x?key=SEEDED".into()],
            notes_l10n: vec![crate::text::Localized::fixed(
                crate::providers::SCHEMA_NOTE,
                "ana@example.com SEEDED",
            )],
            source: "https://secret.example.com/SEEDED".into(),
            session_expires_at: None,
            diagnostic: None,
        }
    }

    fn app() -> AppInfo {
        AppInfo {
            version: "0.80.0".into(),
            os: "macos".into(),
            arch: "aarch64".into(),
            install_channel: "homebrew".into(),
        }
    }

    fn account() -> Account {
        Account {
            name: "ana@example.com SEEDED-NAME".into(),
            provider: Provider::Claude,
            credential_env: Some("SEEDED_CREDENTIAL_ENV".into()),
            params: [("org".to_string(), "SEEDED-ORG".to_string())].into(),
            ..Default::default()
        }
    }

    #[derive(serde::Deserialize, Debug)]
    #[allow(dead_code)]
    struct Window {
        utilization: f64,
    }

    #[test]
    fn issue_urls_are_only_this_projects_form() {
        let draft = app_draft(&app());
        assert!(is_issue_url(&draft.url));
        assert!(!draft.body.contains("SEEDED"));
        for url in [
            "https://github.com/other/repo/issues/new?x=1",
            "https://github.com/jmarcon/cuotai/issues/new?x=1 y",
            "https://github.com/jmarcon/cuotai/pulls",
            "javascript:alert(1)",
            "",
        ] {
            assert!(!is_issue_url(url), "{url}");
        }
    }

    #[test]
    fn the_draft_carries_the_facts_and_none_of_the_seeded_secrets() {
        let account = account();
        let mut failing = report(&account, Status::Error);
        let value = json!({"utilization": "sk-live-SEEDED", "ana@example.com": 1});
        failing.diagnostic = Some(diagnostic::Diagnostic::SchemaDrift(
            diagnostic::decode::<Window>(&value).unwrap_err(),
        ));
        let draft = issue_draft(
            &AccountView {
                account: &account,
                report: &failing,
                last_good_age: Some(Duration::from_secs(7_300)),
                incident_active: Some(false),
            },
            &app(),
        );
        assert_eq!(draft.title, "[Provider] claude: schema_changed");
        for fact in [
            "Cuotai version: 0.80.0",
            "macos aarch64",
            "Install channel: homebrew",
            "Provider: `claude`",
            "Connection mode: `environment_variable`",
        ] {
            assert!(draft.body.contains(fact), "{fact}\n{}", draft.body);
        }
        assert!(draft.body.contains("`utilization` (number -> string)"));
        assert!(draft.body.contains("Last good reading: 2 h ago"));
        assert!(draft.body.contains("Provider incident active: no"));
        assert!(draft
            .body
            .contains("Access level: undocumented (verified on "));
        let decoded = url::Url::parse(&draft.url).unwrap();
        assert_eq!(decoded.path(), "/jmarcon/cuotai/issues/new");
        let pairs: std::collections::BTreeMap<_, _> = decoded.query_pairs().collect();
        assert_eq!(pairs["template"], TEMPLATE);
        assert_eq!(pairs["provider"], "claude");
        assert!(!draft.copy_only);
        for text in [&draft.title, &draft.body, &draft.url] {
            for secret in ["SEEDED", "example.com", "sk-live", "ana@"] {
                assert!(!text.contains(secret), "{secret} leaked in {text}");
            }
        }
        let all = format!("{}{}", draft.body, decoded.query().unwrap());
        assert!(!all.contains("SEEDED"));
    }

    /// RF3.1: the provider dropdown of the issue form lists every provider;
    /// adding one without regenerating fails here.
    #[test]
    fn issue_template_providers() {
        const FILE: &str = ".github/ISSUE_TEMPLATE/provider-problem.yml";
        const START: &str =
            "        # providers:start (generated from Provider::ALL; do not edit by hand)\n";
        const END: &str = "        # providers:end\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(FILE);
        let form = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        let (head, rest) = form.split_once(START).expect("providers start marker");
        let (current, tail) = rest.split_once(END).expect("providers end marker");
        let expected: String = Provider::ALL
            .into_iter()
            .map(|p| format!("        - {}\n", p.slug()))
            .collect();
        if std::env::var_os("CUOTAI_REGENERATE_DOCS").is_some() {
            std::fs::write(&path, format!("{head}{START}{expected}{END}{tail}")).unwrap();
            return;
        }
        assert_eq!(
            current, expected,
            "{FILE} is out of date with Provider::ALL. Regenerate it with \
             `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core issue_template_providers`."
        );
        // RF3.2: every key the draft prefills is a field of the form.
        for key in [
            "provider", "version", "mode", "category", "paths", "context",
        ] {
            assert!(form.contains(&format!("    id: {key}\n")), "field {key}");
        }
    }

    #[test]
    fn hostile_app_fields_stay_plain_and_every_provider_drafts() {
        let hostile = AppInfo {
            version: "1.0 SEEDED\nhttps://x.example/?t=SEEDED".into(),
            os: "".into(),
            arch: "<script>".into(),
            install_channel: "a b".into(),
        };
        for provider in Provider::ALL {
            let account = Account {
                provider,
                ..account()
            };
            let failing = report(&account, Status::Error);
            let draft = issue_draft(
                &AccountView {
                    account: &account,
                    report: &failing,
                    last_good_age: None,
                    incident_active: None,
                },
                &hostile,
            );
            assert!(!draft.body.contains("://"), "{provider:?}");
            assert!(!draft.body.contains("SEEDED-NAME"));
            assert!(draft
                .title
                .starts_with(&format!("[Provider] {}", provider.slug())));
        }
    }

    #[test]
    fn a_very_long_draft_is_offered_only_for_copying() {
        let account = account();
        let mut failing = report(&account, Status::Error);
        let mut drift = diagnostic::decode::<Window>(&json!({"utilization": "x"})).unwrap_err();
        drift.paths = (0..200)
            .map(|i| diagnostic::DriftPath {
                path: format!("{}{i}", "field_".repeat(30)),
                expected: "number".into(),
                found: "string".into(),
            })
            .collect();
        failing.diagnostic = Some(diagnostic::Diagnostic::SchemaDrift(drift));
        let draft = issue_draft(
            &AccountView {
                account: &account,
                report: &failing,
                last_good_age: None,
                incident_active: None,
            },
            &app(),
        );
        assert!(draft.copy_only);
    }
}
