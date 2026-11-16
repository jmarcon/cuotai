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
/// The form of an application problem not tied to an account (About).
pub const BUG_TEMPLATE: &str = "bug.yml";
/// The bare provider-problem form: what a copy-only draft opens.
pub const FORM_URL: &str =
    "https://github.com/jmarcon/cuotai/issues/new?template=provider-problem.yml";
/// The bare application-bug form.
pub const BUG_FORM_URL: &str = "https://github.com/jmarcon/cuotai/issues/new?template=bug.yml";
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
    /// The front end that builds the draft; prefills the form's
    /// `interface` dropdown when known.
    pub interface: Option<Interface>,
}

/// The front end a draft comes from, as the issue forms' `interface`
/// dropdown names it (GitHub prefills a dropdown by its exact label).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interface {
    Desktop,
    Tui,
    Cli,
}

impl Interface {
    /// The exact option label of the forms' `interface` dropdown.
    pub fn label(self) -> &'static str {
        match self {
            Self::Desktop => "Desktop",
            Self::Tui => "TUI",
            Self::Cli => "CLI (--once or --json)",
        }
    }
}

impl AppInfo {
    /// This build's version and platform with the given install channel.
    pub fn current(install_channel: &str) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            install_channel: install_channel.to_string(),
            interface: None,
        }
    }

    /// The same facts, built by `interface`.
    pub fn with_interface(self, interface: Interface) -> Self {
        Self {
            interface: Some(interface),
            ..self
        }
    }
}

/// What is known of the account's last good reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastGood {
    /// Nothing tells (a one-shot query without a recorded reading).
    Unknown,
    /// The running app saw no good reading of this account.
    Never,
    /// A good reading this long ago.
    Ago(Duration),
}

impl LastGood {
    /// From an age the app computed: `None` means it saw none.
    pub fn seen(age: Option<Duration>) -> Self {
        age.map_or(Self::Never, Self::Ago)
    }

    /// For a one-shot query (`--report-problem`): now when `current` is
    /// good, else the age of the last recorded good reading, else unknown.
    pub fn one_shot(
        current: &Report,
        recorded: Option<&Report>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        let good = |report: &Report| report.status == crate::Status::Ready;
        [Some(current), recorded]
            .into_iter()
            .flatten()
            .find(|report| good(report))
            .map_or(Self::Unknown, |report| {
                Self::Ago((now - report.checked_at).to_std().unwrap_or_default())
            })
    }

    fn text(self) -> String {
        match self {
            Self::Unknown => "unknown".into(),
            Self::Never => "none".into(),
            Self::Ago(age) => age_text(age),
        }
    }
}

/// What is known of the failing account.
#[derive(Debug, Clone)]
pub struct AccountView<'a> {
    pub account: &'a Account,
    pub report: &'a Report,
    /// The last good reading (RF2.2).
    pub last_good: LastGood,
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
    /// The bare form ([`FORM_URL`] or [`BUG_FORM_URL`]) a copy-only draft
    /// opens instead.
    pub form: &'static str,
}

impl IssueDraft {
    /// What a front end opens: the prefilled URL, or the bare form when the
    /// URL is too long (RF2.3). Always this project's issue form.
    pub fn link(&self) -> &str {
        if self.copy_only {
            self.form
        } else {
            &self.url
        }
    }
}

impl IssueDraft {
    /// PRD 0046 RF4.4: the draft with the setup check's problems appended:
    /// one line per `fail` or `warn` check, by its stable ids only (the
    /// check and what it found), never a path, variable, host or value, so
    /// the draft stays as private as before. A report without problems
    /// leaves the draft unchanged.
    ///
    /// `account` is the index of the reported account: only its own
    /// `account.<n>` checks are kept beside the global ones; `None` (About)
    /// keeps the global checks only.
    pub fn with_setup_check(self, report: &crate::doctor::Report, account: Option<usize>) -> Self {
        use crate::doctor::CheckStatus;
        let own = account.map(|index| format!("account.{}", index + 1));
        let relevant = |id: &str| match id.strip_prefix("account.") {
            None => true,
            Some(_) => own.as_deref().is_some_and(|own| {
                id == own
                    || id
                        .strip_prefix(own)
                        .is_some_and(|rest| rest.starts_with('.'))
            }),
        };
        let lines: Vec<String> = report
            .checks
            .iter()
            .filter(|check| matches!(check.status, CheckStatus::Fail | CheckStatus::Warn))
            .filter(|check| relevant(&check.id))
            .map(|check| {
                let status = if check.status == CheckStatus::Fail {
                    "fail"
                } else {
                    "warn"
                };
                let found = check
                    .detail
                    .as_ref()
                    .map(|detail| format!(": `{}`", plain_id(&detail.id)))
                    .unwrap_or_default();
                format!("- {status} `{}`{found}\n", plain_id(&check.id))
            })
            .collect();
        if lines.is_empty() {
            return self;
        }
        let section = format!("\n## Setup check (cuotai --doctor)\n\n{}", lines.concat());
        let footer = "\nNo account name, e-mail, organization, credential, URL or response value is included.\n";
        let body = match self.body.strip_suffix(footer) {
            Some(head) => format!("{head}{section}{footer}"),
            None => format!("{}{section}", self.body),
        };
        let Some((base, query)) = self.url.split_once('?') else {
            return Self { body, ..self };
        };
        let mut rebuilt = url::form_urlencoded::Serializer::new(String::new());
        for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
            if key == "context" {
                rebuilt.append_pair("context", &body);
            } else {
                rebuilt.append_pair(&key, &value);
            }
        }
        let url = format!("{base}?{}", rebuilt.finish());
        let copy_only = url.chars().count() > MAX_URL_CHARS;
        Self {
            title: self.title,
            body,
            url,
            copy_only,
            form: self.form,
        }
    }
}

/// A stable id kept to identifier characters (`account.2`,
/// `doctor.account.env_missing`).
fn plain_id(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .take(64)
        .collect()
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

/// How the account authenticates, without naming any credential: the
/// account's own login, vault session or environment variable first, then
/// the interface's credential (a local CLI, an agent feed, an aggregator
/// without a password), else `none`.
pub(crate) fn auth_mode(account: &Account) -> &'static str {
    use access::Credential;
    if account.provider == crate::Provider::Extension {
        return "extension";
    }
    if account.oauth.is_some() {
        return "native_login";
    }
    if account.session.is_some() {
        return "session_in_vault";
    }
    let env = account
        .credential_env
        .as_deref()
        .is_some_and(|name| !name.trim().is_empty());
    match access::describe(account).credential {
        Credential::LocalCli => "local_cli",
        Credential::AgentFeed => "agent_feed",
        Credential::AggregatorLogin if env => "aggregator_password",
        Credential::AggregatorLogin => "none",
        Credential::CloudIdentity if !env => "cloud_identity",
        _ if env => "environment_variable",
        _ => "none",
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

/// Whether `url` is one of this project's bare issue forms: the only
/// problem-report address a webview may hand to the system browser. A
/// prefilled draft is rebuilt and opened by the shell itself, so no query
/// chosen by the page is ever opened (PRD 0028 re-validation, S-01).
pub fn is_bare_form_url(url: &str) -> bool {
    url == FORM_URL || url == BUG_FORM_URL
}

/// The draft of a problem that is not tied to an account (About): only the
/// application facts, with the issue form opened for the rest.
pub fn app_draft(app: &AppInfo) -> IssueDraft {
    let version = plain(&app.version);
    let os = plain(&app.os);
    let arch = plain(&app.arch);
    let channel = plain(&app.install_channel);
    let title = "[Bug] ".to_string();
    let body = format!(
        "## Problem report\n\n- Cuotai version: {version}\n- OS and architecture: {os} {arch}\n- Install channel: {channel}\n\nNo account name, e-mail, organization, credential, URL or response value is included.\n"
    );
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("template", BUG_TEMPLATE);
    query.append_pair("title", &title);
    query.append_pair("version", &version);
    if let Some(interface) = app.interface {
        query.append_pair("interface", interface.label());
    }
    query.append_pair("os", &format!("{os} {arch}"));
    query.append_pair("context", &body);
    let url = format!("{REPOSITORY}/issues/new?{}", query.finish());
    let copy_only = url.chars().count() > MAX_URL_CHARS;
    IssueDraft {
        title,
        body,
        url,
        copy_only,
        form: BUG_FORM_URL,
    }
}

/// Keep a canary shape diff to plain, bounded single-line text: control
/// characters become spaces; bidi controls and other invisible format
/// characters are dropped (PRD 0028 re-validation, S-04).
fn plain_diff(diff: &str) -> String {
    diff.chars()
        .filter(|c| !is_invisible_format(*c))
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(1_500)
        .collect()
}

/// A bidi control (U+061C, U+200E/F, U+202A–U+202E, U+2066–U+2069) or
/// another invisible format character (zero-width, word joiner, BOM, soft
/// hyphen).
pub(crate) fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2069}'
            | '\u{FEFF}'
    )
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
    line(format!("Last good reading: {}", view.last_good.text()));
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
    if let Some(interface) = app.interface {
        query.append_pair("interface", interface.label());
    }
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
        form: FORM_URL,
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
            breakdown: Vec::new(),
        }
    }

    fn app() -> AppInfo {
        AppInfo {
            version: "0.80.0".into(),
            os: "macos".into(),
            arch: "aarch64".into(),
            install_channel: "homebrew".into(),
            interface: Some(Interface::Tui),
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
    fn the_setup_check_adds_its_problems_by_id_only() {
        use crate::doctor::{Check, CheckStatus, Report};
        use crate::text::Localized;
        let check = |id: &str, status, detail: Option<Localized>| Check {
            id: id.into(),
            status,
            title: Localized::with_param("t", "path", "~/SEEDED/cuotai.toml", "{path}"),
            detail,
            fix: Some(Localized::with_param(
                "f",
                "variable",
                "SEEDED_KEY",
                "Set {variable}",
            )),
        };
        let report = Report {
            schema: 1,
            checks: vec![
                check("version", CheckStatus::Ok, None),
                check(
                    "account.2",
                    CheckStatus::Fail,
                    Some(Localized::with_param(
                        "doctor.account.env_missing",
                        "variable",
                        "SEEDED_KEY",
                        "It reads {variable}",
                    )),
                ),
                check("port", CheckStatus::Warn, None),
            ],
            tools: Default::default(),
            artifacts: Vec::new(),
        };
        // About keeps only the global checks.
        let about = app_draft(&app()).with_setup_check(&report, None);
        assert!(about
            .body
            .contains("## Setup check (cuotai --doctor)\n\n- warn `port`\n"));
        assert!(!about.body.contains("account.2"));
        // A report of another account leaves account.2 out too (C-15).
        let other = app_draft(&app()).with_setup_check(&report, Some(0));
        assert!(!other.body.contains("account.2"));
        // The reported account (index 1 is `account.2`) keeps its own.
        let draft = app_draft(&app()).with_setup_check(&report, Some(1));
        assert!(draft.body.contains(
            "## Setup check (cuotai --doctor)\n\n- fail `account.2`: `doctor.account.env_missing`\n- warn `port`\n"
        ));
        assert!(draft.body.ends_with("response value is included.\n"));
        assert!(!draft.body.contains("SEEDED") && !draft.url.contains("SEEDED"));
        assert!(draft.url.contains("Setup+check"));
        // Nothing to add: the draft is unchanged.
        let clean = Report {
            schema: 1,
            checks: vec![check("version", CheckStatus::Ok, None)],
            tools: Default::default(),
            artifacts: Vec::new(),
        };
        assert_eq!(
            app_draft(&app()).with_setup_check(&clean, Some(1)),
            app_draft(&app())
        );
    }

    /// S-01: only the two bare forms may be handed to the browser; a
    /// prefilled URL (any query the page could choose) never.
    #[test]
    fn issue_urls_are_only_this_projects_bare_forms() {
        assert!(is_bare_form_url(FORM_URL));
        assert!(is_bare_form_url(BUG_FORM_URL));
        let draft = app_draft(&app());
        assert!(!is_bare_form_url(&draft.url));
        assert!(!draft.body.contains("SEEDED"));
        for url in [
            "https://github.com/jmarcon/cuotai/issues/new?template=provider-problem.yml&title=x&body=y",
            "https://github.com/jmarcon/cuotai/issues/new?title=Phish&body=run+this",
            "https://github.com/other/repo/issues/new?x=1",
            "https://github.com/jmarcon/cuotai/pulls",
            "javascript:alert(1)",
            "",
        ] {
            assert!(!is_bare_form_url(url), "{url}");
        }
    }

    /// C-14: About opens the bug form, titled and prefilled as a bug.
    #[test]
    fn the_about_draft_opens_the_bug_form() {
        let draft = app_draft(&app());
        assert_eq!(draft.title, "[Bug] ");
        assert_eq!(draft.form, BUG_FORM_URL);
        let decoded = url::Url::parse(&draft.url).unwrap();
        let pairs: std::collections::BTreeMap<_, _> = decoded.query_pairs().collect();
        assert_eq!(pairs["template"], BUG_TEMPLATE);
        assert_eq!(pairs["title"], "[Bug] ");
        assert_eq!(pairs["version"], "0.80.0");
        assert_eq!(pairs["os"], "macos aarch64");
        assert_eq!(pairs["interface"], "TUI");
        assert!(pairs["context"].contains("Install channel: homebrew"));
    }

    /// C-01: the one-shot path says "unknown" when nothing tells, and the
    /// app path "none" only when it saw no good reading.
    #[test]
    fn the_last_good_reading_is_known_never_or_unknown() {
        let account = account();
        let now = chrono::Utc::now();
        let failing = report(&account, Status::Error);
        let mut good = report(&account, Status::Ready);
        good.checked_at = now - chrono::Duration::minutes(7);
        assert_eq!(LastGood::one_shot(&failing, None, now), LastGood::Unknown);
        assert_eq!(
            LastGood::one_shot(&failing, Some(&good), now),
            LastGood::Ago(Duration::from_secs(420))
        );
        assert_eq!(LastGood::seen(None), LastGood::Never);
        let draft = |last_good| {
            issue_draft(
                &AccountView {
                    account: &account,
                    report: &failing,
                    last_good,
                    incident_active: None,
                },
                &app(),
            )
            .body
        };
        assert!(draft(LastGood::Unknown).contains("Last good reading: unknown"));
        assert!(draft(LastGood::Never).contains("Last good reading: none"));
        assert!(
            draft(LastGood::Ago(Duration::from_secs(420))).contains("Last good reading: 7 min ago")
        );
    }

    /// C-11: an account without a credential is not "environment_variable".
    #[test]
    fn the_connection_mode_follows_the_credential() {
        let bare = |provider| Account {
            provider,
            credential_env: None,
            ..Default::default()
        };
        assert_eq!(auth_mode(&account()), "environment_variable");
        assert_eq!(auth_mode(&bare(Provider::NineRouter)), "none");
        assert_eq!(
            auth_mode(&Account {
                credential_env: Some("NINE_ROUTER_PASSWORD".into()),
                ..bare(Provider::NineRouter)
            }),
            "aggregator_password"
        );
        assert_ne!(auth_mode(&bare(Provider::Claude)), "environment_variable");
    }

    /// S-04: bidi and other invisible format characters never reach a draft.
    #[test]
    fn a_shape_diff_loses_bidi_and_control_characters() {
        let diff = plain_diff("a\u{202e}b \u{1b}[31m c\u{2066}d\u{200b}e");
        assert_eq!(diff, "ab  [31m cde");
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
                last_good: LastGood::Ago(Duration::from_secs(7_300)),
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
        assert_eq!(pairs["interface"], "TUI");
        assert_eq!(draft.form, FORM_URL);
        assert!(!draft.copy_only);
        for text in [&draft.title, &draft.body, &draft.url] {
            for needle in ["SEEDED", "example.com", "sk-live", "ana@"] {
                assert!(!text.contains(needle), "{needle} leaked in {text}");
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
            "provider",
            "version",
            "mode",
            "category",
            "paths",
            "context",
            "interface",
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
            interface: None,
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
                    last_good: LastGood::Never,
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
                last_good: LastGood::Never,
                incident_active: None,
            },
            &app(),
        );
        assert!(draft.copy_only);
    }
}
