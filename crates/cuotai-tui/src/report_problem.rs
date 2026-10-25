//! PRD 0028 RF2.4: `cuotai --report-problem ACCOUNT`. One query of the
//! account, then the secret-free draft of `cuotai_core::support` on stdout.
//! Nothing is sent: the user opens the URL.

use cuotai_core::{
    install_source::detect_install_source,
    oauth::OAuthSession,
    support::{issue_draft, AccountView, AppInfo, IssueDraft},
    Config, EnvironmentCredentials, Monitor, Report, Status,
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
                .position(|account| account.name.eq_ignore_ascii_case(name))
        })
}

/// The printable text: title, body and the link (or the copy-only notice).
pub fn render(draft: &IssueDraft, healthy: bool) -> String {
    let mut out = String::new();
    if healthy {
        out.push_str(
            "Note: the account answered normally; report it only if its values look wrong.\n\n",
        );
    }
    out.push_str(&format!("Title: {}\n\n{}\n", draft.title, draft.body));
    if draft.copy_only {
        out.push_str("\nThe prefilled link is too long to open; copy the text above into a new issue:\nhttps://github.com/jmarcon/cuotai/issues/new?template=provider-problem.yml\n");
    } else {
        out.push_str(&format!(
            "\nOpen this link to file the issue (nothing is sent by Cuotai):\n{}\n",
            draft.url
        ));
    }
    out
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
    name: &str,
    session: Arc<OAuthSession>,
) -> Result<u8, Box<dyn std::error::Error>> {
    let Some(index) = find(config, name) else {
        eprintln!("No account with that name in the configuration.");
        return Ok(1);
    };
    let monitor = Monitor::with_oauth(config.clone(), Arc::new(EnvironmentCredentials), session)?;
    let report: Report = monitor.refresh_account(index).await;
    let draft = issue_draft(
        &AccountView {
            account: &config.accounts[index],
            report: &report,
            last_good_age: None,
            // Status queries are off in this one-shot path unless enabled
            // elsewhere; the helper then reports "unknown".
            incident_active: cuotai_core::incidents::incident_active(
                &monitor.incident_statuses(),
                config.accounts[index].provider,
            ),
        },
        &app_info(),
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
        };
        let text = render(&draft, false);
        assert!(text.starts_with("Title: [Provider] claude: schema_changed"));
        assert!(text.contains("issues/new?x=1"));
        let long = render(
            &IssueDraft {
                copy_only: true,
                ..draft
            },
            true,
        );
        assert!(long.starts_with("Note:") && !long.contains("x=1"));
    }
}
