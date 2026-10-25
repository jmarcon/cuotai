//! PRD 0028 RF8.2 / RF8.3: the opt-in update notice of the CLI.
//!
//! The policy (what is asked, how often, what is cached, the wording) lives
//! in `cuotai_core::update_check`. This module only decides *when* to run it
//! and *where* the line goes: standard error, after the run, and only for the
//! interactive TUI and `--once` on a terminal. It never runs for `--json`,
//! `--json-v1`, `--watch`, `--serve` or when standard output is not a
//! terminal, and a failing check is silent.

use chrono::Utc;
use cuotai_core::{
    config_file::ConfigFile,
    install_source::detect_install_source,
    update_check::{self, Version},
};
use std::io::IsTerminal;
use tokio::task::JoinHandle;

pub struct Pending(JoinHandle<Option<Version>>);

/// Start the check in the background when `wanted` (the run is interactive or
/// `--once`) and the user opted in. Nothing is requested otherwise.
pub fn start(file: &ConfigFile, wanted: bool) -> Option<Pending> {
    start_with(
        file.updates().check,
        std::env::var("CUOTAI_UPDATE_CHECK").ok().as_deref(),
        wanted && std::io::stdout().is_terminal(),
    )
}

fn start_with(setting: bool, environment: Option<&str>, wanted: bool) -> Option<Pending> {
    if !wanted || !update_check::enabled(environment, setting) {
        return None;
    }
    let cache = update_check::cache_path();
    Some(Pending(tokio::spawn(async move {
        update_check::newer_version(
            update_check::MANIFEST_URL,
            cache.as_deref(),
            Utc::now(),
            env!("CARGO_PKG_VERSION"),
        )
        .await
    })))
}

/// Print the notice, when the check found a newer version, to standard error.
pub async fn print(pending: Option<Pending>, locale: &str) {
    let Some(Pending(handle)) = pending else {
        return;
    };
    let Some(latest) = handle.await.ok().flatten() else {
        return;
    };
    let executable = std::env::current_exe().unwrap_or_default();
    let source = detect_install_source(
        &executable,
        |key| std::env::var_os(key),
        // The notice only names a command; whether the directory is writable
        // does not change it.
        || true,
    );
    eprintln!("{}", update_check::notice(latest, source).resolve(locale));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn nothing_starts_unless_wanted_and_opted_in() {
        assert!(start_with(false, None, true).is_none(), "off by default");
        assert!(
            start_with(true, None, false).is_none(),
            "not an eligible run"
        );
        assert!(start_with(true, Some("0"), true).is_none());
        assert!(start_with(false, Some("1"), false).is_none());
    }
}
