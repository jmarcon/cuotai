//! PRD 0028 RF4: `cuotai --canary`. One refresh of the owner's own accounts,
//! a value-free shape record, drift against the previous run, a Markdown
//! report, issue drafts and the list of interfaces that answered cleanly.
//! The canary never files anything and never writes outside its directory.

use chrono::Utc;
use cuotai_core::{
    canary::{self, Store},
    oauth::OAuthSession,
    Config, EnvironmentCredentials, Monitor,
};
use std::path::PathBuf;
use std::sync::Arc;

pub struct CanaryArgs {
    pub out: Option<PathBuf>,
    pub dry_run: bool,
    pub open: bool,
    pub include_metered: bool,
    /// Also check the status components (RF4.5).
    pub status: bool,
}

/// The configuration without the accounts that are billed per query, and
/// their provider slugs (RF4.6).
pub fn without_metered(config: &Config, include_metered: bool) -> (Config, Vec<String>) {
    if include_metered {
        return (config.clone(), Vec::new());
    }
    let mut skipped: Vec<String> = Vec::new();
    let mut kept = config.clone();
    kept.accounts.retain(|account| {
        let metered = cuotai_core::access::describe(account).metered;
        if metered {
            let slug = account.provider.slug().to_string();
            if !skipped.contains(&slug) {
                skipped.push(slug);
            }
        }
        !metered
    });
    (kept, skipped)
}

pub async fn run(
    config: &Config,
    args: CanaryArgs,
    session: Arc<OAuthSession>,
) -> Result<u8, Box<dyn std::error::Error>> {
    let store = match args.out {
        Some(dir) => Store::new(dir),
        None => Store::in_data_dir().ok_or(crate::l10n::tr(
            "canary.no_data_dir",
            "No local data directory for the canary.",
        ))?,
    };
    let (config, skipped) = without_metered(config, args.include_metered);
    if config.accounts.is_empty() {
        eprintln!("No account to observe (metered accounts need --canary-include-metered).");
        return Ok(1);
    }
    let monitor =
        Monitor::with_login_session(config.clone(), Arc::new(EnvironmentCredentials), session)?;
    let (snapshot, responses) = monitor.refresh_with_shapes().await;
    let now = Utc::now();
    let date = now.format("%Y-%m-%d").to_string();
    let previous = store.latest();
    let app = crate::report_problem::app_info();
    // RF4.5: public status pages are contacted only when status queries are
    // on (`--status` / CUOTAI_STATUS), like everywhere else (PRD 0011 RF5).
    let status_missing = if args.status {
        monitor.missing_status_components().await
    } else {
        Vec::new()
    };
    // PRD 0028 RF2.2: the drafts state whether an incident is active, from
    // the same consented status queries.
    let statuses = if args.status {
        monitor.incidents().set_enabled(true);
        monitor.refresh_incidents().await
    } else {
        Vec::new()
    };
    let outcome = canary::analyze_with(
        previous.as_ref(),
        &config.accounts,
        &snapshot.reports,
        &responses,
        &status_missing,
        &statuses,
        &date,
        &skipped,
        &app,
    );
    print!("{}", outcome.markdown);
    for draft in &outcome.drafts {
        println!(
            "\n---\nIssue draft (nothing is filed by the canary)\n\n{}",
            crate::report_problem::render(draft, false)
        );
    }
    if args.open {
        for draft in outcome.drafts.iter().filter(|d| !d.copy_only) {
            if !cuotai_core::oauth::login::open_browser(&draft.url).await {
                eprintln!("Could not open the browser; copy the link above.");
            }
        }
    }
    if args.dry_run {
        eprintln!("Dry run: nothing was written.");
    } else {
        store
            .save(
                &now.format("%Y%m%dT%H%M%SZ").to_string(),
                &outcome.run,
                &outcome.markdown,
                &outcome.evidence,
            )
            .map_err(|_| "Could not write the canary directory.")?;
        eprintln!("Canary records written to {}", store.dir().display());
    }
    Ok(if outcome.drifts.is_empty() { 0 } else { 2 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuotai_core::{Account, Provider};

    #[test]
    fn metered_accounts_are_skipped_unless_included() {
        let config = Config {
            accounts: [Provider::AwsBedrock, Provider::Claude, Provider::AwsBedrock]
                .map(|provider| Account {
                    provider,
                    ..Default::default()
                })
                .into(),
            ..Default::default()
        };
        let (kept, skipped) = without_metered(&config, false);
        assert_eq!(kept.accounts.len(), 1);
        assert_eq!(skipped, ["aws_bedrock"]);
        let (all, none) = without_metered(&config, true);
        assert_eq!(all.accounts.len(), 3);
        assert!(none.is_empty());
    }
}
