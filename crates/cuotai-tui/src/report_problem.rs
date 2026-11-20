//! PRD 0028 RF2.4: `cuotai --report-problem ACCOUNT`. One query of the
//! account, then the secret-free draft of `cuotai_core::support` on stdout.
//! Nothing is sent: the user opens the URL.

use cuotai_core::{
    install_source::detect_install_source,
    oauth::OAuthSession,
    support::{issue_draft, AccountView, AppInfo, Interface, IssueDraft, LastGood},
    Account, Config, EnvironmentCredentials, Monitor, Report, Status,
};
use std::sync::Arc;

/// The account called `name`: exact first, then ignoring case.
pub fn find(config: &Config, name: &str) -> Option<usize> {
    config
        .accounts
        .iter()
        .position(|account| account.name == name)
        .or_else(|| {
            config
                .accounts
                .iter()
                .position(|account| cuotai_core::account_names_match(&account.name, name))
        })
}

/// The printable text: title, body and the link (or the copy-only notice),
/// in the TUI locale. The draft itself stays English (issue text is English
/// per the repository rules); the lines around it are localized.
pub fn render(draft: &IssueDraft, healthy: bool) -> String {
    render_in(crate::ui::tui_locale(), draft, healthy)
}

/// [`render`] in `locale`.
pub(crate) fn render_in(locale: &str, draft: &IssueDraft, healthy: bool) -> String {
    use crate::l10n::tr_in;
    let mut out = String::new();
    if healthy {
        out.push_str(tr_in(
            locale,
            "report.healthy",
            "Note: the account answered normally; report it only if its values look wrong.",
        ));
        out.push_str("\n\n");
    }
    out.push_str(&format!("Title: {}\n\n{}\n", draft.title, draft.body));
    let intro = if draft.copy_only {
        tr_in(
            locale,
            "report.copy_only",
            "The prefilled link is too long to open; copy the text above into a new issue:",
        )
    } else {
        tr_in(
            locale,
            "report.open_link",
            "Open this link to file the issue (nothing is sent by Cuotai):",
        )
    };
    out.push_str(&format!("\n{intro}\n{}\n", draft.link()));
    out
}

/// DS-CMP-DIAG-1 in the terminal: the "What changed" block of a drifted
/// report, its texts resolved in `locale`.
pub(crate) struct DriftTexts {
    pub heading: String,
    pub sentence: String,
    /// `path (expected → found)`, at most eight, plain (no backticks).
    pub paths: Vec<String>,
    pub privacy: String,
}

/// The block of `report`, when its query left a drift.
pub(crate) fn drift_texts(report: &Report, locale: &str) -> Option<DriftTexts> {
    let view = report.diagnostic.as_ref()?.view();
    Some(DriftTexts {
        heading: view.heading.resolve(locale),
        sentence: view.sentence.resolve(locale),
        paths: view
            .paths
            .iter()
            .map(|p| {
                crate::ui::safe(&format!("{} ({} → {})", p.path, p.expected, p.found)).into_owned()
            })
            .collect(),
        privacy: view.privacy.resolve(locale),
    })
}

/// Whether note `index` of `report` is the drift sentence the block above
/// already shows.
pub(crate) fn is_drift_note(report: &Report, index: usize) -> bool {
    report.diagnostic.is_some()
        && report
            .notes_l10n
            .get(index)
            .is_some_and(|note| cuotai_core::diagnostic::Diagnostic::NOTE_IDS.contains(&&*note.id))
}

/// The draft of account `index` (RF2.2), with the setup check's problems
/// of that account and the global ones (PRD 0046 RF4.4), by id only. Shared
/// by the TUI's `b` and `--report-problem`.
pub(crate) fn draft_for(
    account: &Account,
    index: usize,
    report: &Report,
    last_good: LastGood,
    incident_active: Option<bool>,
    interface: Interface,
    config_path: Option<std::path::PathBuf>,
) -> IssueDraft {
    issue_draft(
        &AccountView {
            account,
            report,
            last_good,
            incident_active,
        },
        &app_info().with_interface(interface),
    )
    .with_setup_check(
        &crate::doctor::report_for(config_path, "en-US"),
        Some(index),
    )
}

pub(crate) fn app_info() -> AppInfo {
    let channel = std::env::current_exe()
        .map(|executable| {
            detect_install_source(&executable, |key| std::env::var_os(key), || false).as_str()
        })
        .unwrap_or("unknown");
    AppInfo::current(channel)
}

pub async fn run(
    config: &Config,
    config_path: &std::path::Path,
    name: &str,
    session: Arc<OAuthSession>,
) -> Result<u8, Box<dyn std::error::Error>> {
    let Some(index) = find(config, name) else {
        crate::cli::message(
            crate::cli::Level::Warn,
            "report.no_account",
            crate::l10n::tr(
                "report.no_account",
                "No account with that name in the configuration.",
            ),
        );
        return Ok(1);
    };
    let monitor =
        Monitor::with_login_session(config.clone(), Arc::new(EnvironmentCredentials), session)?;
    let report: Report = monitor.refresh_account(index).await;
    // C-01: one query and no running cycle: the last recorded good reading
    // when history has one, else "unknown" (never "none").
    let recorded = monitor.restored_readings().into_iter().nth(index).flatten();
    let last_good = LastGood::one_shot(&report, recorded.as_ref(), chrono::Utc::now());
    let draft = draft_for(
        &config.accounts[index],
        index,
        &report,
        last_good,
        // Status queries are off in this one-shot path unless enabled
        // elsewhere; the helper then reports "unknown".
        cuotai_core::incidents::incident_active(
            &monitor.incident_statuses(),
            &config.accounts[index],
        ),
        Interface::Cli,
        Some(config_path.to_path_buf()),
    );
    print!("{}", render(&draft, report.status == Status::Ready));
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuotai_core::Account;

    fn config() -> Config {
        Config {
            accounts: ["Work", "personal"]
                .map(|name| Account {
                    name: name.into(),
                    ..Default::default()
                })
                .into(),
            ..Default::default()
        }
    }

    #[test]
    fn accounts_are_found_by_exact_name_then_ignoring_case() {
        assert_eq!(find(&config(), "Work"), Some(0));
        assert_eq!(find(&config(), "PERSONAL"), Some(1));
        assert_eq!(find(&config(), "other"), None);
    }

    #[test]
    fn the_output_prints_the_draft_and_the_link() {
        let draft = IssueDraft {
            title: "[Provider] claude: schema_changed".into(),
            body: "- Provider: `claude`\n".into(),
            url: "https://github.com/jmarcon/cuotai/issues/new?x=1".into(),
            copy_only: false,
            form: cuotai_core::support::FORM_URL,
        };
        let text = render_in("en-US", &draft, false);
        assert!(text.starts_with("Title: [Provider] claude: schema_changed"));
        assert!(text.contains("issues/new?x=1"));
        let long = render_in(
            "en-US",
            &IssueDraft {
                copy_only: true,
                ..draft
            },
            true,
        );
        assert!(long.starts_with("Note:") && !long.contains("x=1"));
        assert!(long.contains("issues/new?template=provider-problem.yml"));
    }

    /// C-12: the lines around the draft follow the locale; the draft stays
    /// English.
    #[test]
    fn the_lines_around_the_draft_are_localized() {
        let draft = IssueDraft {
            title: "[Provider] claude: schema_changed".into(),
            body: "- Provider: `claude`\n".into(),
            url: "https://github.com/jmarcon/cuotai/issues/new?x=1".into(),
            copy_only: false,
            form: cuotai_core::support::FORM_URL,
        };
        let text = render_in("pt-BR", &draft, true);
        assert!(!text.contains("Note: the account"), "{text}");
        assert!(!text.contains("Open this link"), "{text}");
        assert!(text.contains("- Provider: `claude`"));
        let long = render_in(
            "pt-BR",
            &IssueDraft {
                copy_only: true,
                ..draft
            },
            false,
        );
        assert!(!long.contains("too long to open"), "{long}");
    }

    /// C-01: a failing account that read well before says how long ago.
    #[test]
    fn the_draft_names_the_last_good_reading() {
        let config = config();
        let report = Report {
            account: "Work".into(),
            account_id: None,
            provider: config.accounts[0].provider,
            checked_at: chrono::Utc::now(),
            status: Status::Error,
            metrics: Vec::new(),
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            diagnostic: None,
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
            breakdown: Vec::new(),
        };
        let draft = draft_for(
            &config.accounts[0],
            0,
            &report,
            LastGood::seen(Some(std::time::Duration::from_secs(180))),
            None,
            Interface::Tui,
            None,
        );
        assert!(
            draft.body.contains("Last good reading: 3 min ago"),
            "{}",
            draft.body
        );
        assert!(draft.url.contains("interface=TUI"));
    }
}
