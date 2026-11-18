//! PRD 0020 re-validation S-01, PRD 0050 re-validation S-01/S-02: the
//! windows each desktop command may come from.
//!
//! The three webviews (`main`, `tray`, `settings`) share one capability, so
//! Tauri lets each of them invoke every command. A command that changes data,
//! a consent or privacy choice, a secret, or starts a sign-in therefore checks
//! the label of the window that called it, first, before it touches anything:
//!
//! - the account dialogs and the main window's own account actions run only
//!   from the main window ([`MAIN_ONLY`]);
//! - a restore, the extension dialog, the update answer and the other actions
//!   both windows offer run from the main window or Settings
//!   ([`MAIN_OR_SETTINGS`]);
//! - every Settings command that erases, writes a setting, a consent or
//!   privacy choice or a secret, or sends anything off the device is
//!   Settings-only ([`SETTINGS_ONLY`]).
//!
//! The tray popover may call none of them. A refusal is the fixed,
//! translated `desktop.error.wrong_window` error, and nothing is done.
//!
//! Every command of `generate_handler!` is classified exactly once, either in
//! [`SCOPED`] or in [`ANY_WINDOW`] with one of the fixed reasons of
//! [`REASONS`]; a command missing from both fails
//! `lib.rs::every_command_has_a_window_scope`. The rule for a new command:
//!
//! - its callers (the `ui/js` wrapper and every module that imports it) are
//!   all under `js/settings/**` and it writes `desktop.json`, the TOML, a vault
//!   entry or a file, or sends anything off the device: `SETTINGS_ONLY`;
//! - its callers are only the main window's modules and it edits accounts or
//!   their alerts: `MAIN_ONLY`; both of these windows: `MAIN_OR_SETTINGS`;
//! - anything the tray popover calls, every read-only view, a refresh, a
//!   presentation preference, window control or an allow-listed external open
//!   goes to [`ANY_WINDOW`] with its reason.

use tauri::{Runtime, WebviewWindow};

/// The main window's label.
pub const MAIN: &str = crate::main_window::LABEL;
/// The Settings window's label.
pub const SETTINGS: &str = crate::settings_window::LABEL;
/// The tray popover's label (never allowed here; named for the tests).
#[cfg(test)]
pub const TRAY: &str = "tray";

/// Commands of the account dialogs and the main window's account actions.
pub const MAIN_ONLY: &[&str] = &[MAIN];
/// Commands of a Settings page.
pub const SETTINGS_ONLY: &[&str] = &[SETTINGS];
/// Restore, the extension dialog and the update answer, offered by the main
/// window and by Settings.
pub const MAIN_OR_SETTINGS: &[&str] = &[MAIN, SETTINGS];

/// The scoped commands: (the scope's name in this module, its windows, the
/// commands). Each of them calls
/// `window_scope::require(&window, window_scope::<name>)?` as its first
/// statement.
#[cfg_attr(not(test), allow(dead_code))] // Read by the tests and reviewers.
pub(crate) const SCOPED: &[(&str, &[&str], &[&str])] = &[
    (
        "MAIN_ONLY",
        MAIN_ONLY,
        &[
            // The account dialogs (PRD 0020, 0047, 0048 re-validations).
            "cancel_session_capture",
            "capture_session",
            "delete_account",
            "forget_extension_secrets",
            "forget_session",
            "forget_shared_capture",
            "migrate_session",
            "retry_purge",
            "save_account",
            "test_aggregator",
            "test_session",
            // PRD 0025 re-validation S-05: the first-run consent.
            "first_run_turn_on",
            // PRD 0050 re-validation S-01: a sign-in starts only there.
            "login_start",
            "login_submit_code",
            "login_cancel",
            // PRD 0050 re-validation S-02: the main window's account actions.
            "set_connection_flag",
            "history_relocation",
            "set_account_notify",
            "set_pool_alerts",
            "set_breakdown_alerts",
            "acknowledge_missed",
            "updater_install",
            "updater_restart",
        ],
    ),
    (
        "MAIN_OR_SETTINGS",
        MAIN_OR_SETTINGS,
        &[
            "restore_apply",
            "first_run_record",
            // PRD 0050 re-validation S-01: the extension dialog and the update
            // answer are shown by both windows.
            "disable_extension",
            "set_extension_override",
            "set_extension_secret",
            "updater_answer_automatic",
            // PRD 0050 re-validation S-02.
            "enable_extension",
            "test_extension",
            "backup_write",
            "restore_plan",
            "move_account",
            "set_detect_enabled",
        ],
    ),
    (
        "SETTINGS_ONLY",
        SETTINGS_ONLY,
        &[
            "history_erase",
            "cost_erase",
            // PRD 0042 re-validation S-08.
            "cost_breakdown",
            "cost_set_label",
            "cost_set_hidden",
            "cost_set_source_options",
            "vault_prune",
            "vault_remove",
            "erase_all_data",
            "erase_serve_access_log",
            "generate_serve_token",
            "revoke_serve_token",
            "copy_serve_token",
            "delete_widget_data",
            "channel_remove",
            // PRD 0044 re-validation S-05.
            "channel_add",
            "channel_edit",
            "channel_set_enabled",
            "channel_test",
            "set_config_file",
            "set_data_dir",
            "set_user_dir",
            // PRD 0050 re-validation S-01: erase, consent and privacy switches.
            "forget_claude_code_readings",
            "cost_disable",
            "cost_set_paths",
            "cost_set_options",
            "set_history_days",
            "set_serve",
            "set_serve_stats",
            "set_serve_access_log",
            "set_widgets",
            "set_notify_private",
            "autostart_set",
            // PRD 0050 re-validation S-02: the other Settings writers.
            "autostart_repair",
            "set_refresh_seconds",
            "set_live_session",
            "set_history_recording",
            "set_status_enabled",
            "history_export",
            "set_hooks_enabled",
            "confirm_hooks",
            "save_hooks",
            "test_hook",
            "cost_enable",
            "cost_set_plan",
            "set_notify_enabled",
            "set_alerts_enabled",
            "set_projected_alert",
            "set_alert_options",
            "set_notify_options",
            "set_quiet_hours",
            "send_test_notification",
            "set_insight_prefs",
            "send_test_digest",
            "updater_set_automatic",
            "updater_check_now",
        ],
    ),
];

/// A view that reads and changes nothing.
#[cfg_attr(not(test), allow(dead_code))] // Read by the tests and reviewers.
pub(crate) const READ_ONLY: &str = "read-only view";
/// Runs the queries again or reloads what is on disk; changes no setting.
#[cfg_attr(not(test), allow(dead_code))] // Read by the tests and reviewers.
pub(crate) const REFRESH: &str = "refresh or reload";
/// A display choice of the window that sends it (density, motion, language,
/// hidden or reordered rows), shared by every window.
#[cfg_attr(not(test), allow(dead_code))] // Read by the tests and reviewers.
pub(crate) const PRESENTATION: &str = "presentation preference of the calling window";
/// Changes only the window that called it.
#[cfg_attr(not(test), allow(dead_code))] // Read by the tests and reviewers.
pub(crate) const CALLING_WINDOW: &str = "acts on the calling window";
/// Shows, opens, restarts or quits a window or the app.
#[cfg_attr(not(test), allow(dead_code))] // Read by the tests and reviewers.
pub(crate) const WINDOW_CONTROL: &str = "window control";
/// Opens a fixed folder or an allow-listed address outside the app.
#[cfg_attr(not(test), allow(dead_code))] // Read by the tests and reviewers.
pub(crate) const EXTERNAL_OPEN: &str = "allow-listed external open";
/// An action of the main window or the tray popover that changes no data.
#[cfg_attr(not(test), allow(dead_code))] // Read by the tests and reviewers.
pub(crate) const NO_DATA: &str = "main-window or tray action that changes no data";
/// A per-metric alert switch, offered by the main window and the tray popover.
#[cfg_attr(not(test), allow(dead_code))] // Read by the tests and reviewers.
pub(crate) const MAIN_AND_TRAY_ALERT: &str = "alert switch of the main window and the tray";

/// The reasons an [`ANY_WINDOW`] entry may give.
#[cfg_attr(not(test), allow(dead_code))] // Read by the tests and reviewers.
pub(crate) const REASONS: &[&str] = &[
    READ_ONLY,
    REFRESH,
    PRESENTATION,
    CALLING_WINDOW,
    WINDOW_CONTROL,
    EXTERNAL_OPEN,
    NO_DATA,
    MAIN_AND_TRAY_ALERT,
];

/// The commands any window may call, each with its reason.
#[cfg_attr(not(test), allow(dead_code))] // Read by the tests and reviewers.
pub(crate) const ANY_WINDOW: &[(&str, &[&str])] = &[
    (
        READ_ONLY,
        &[
            "get_view",
            "tray_preview",
            "tray_accounts",
            "autostart_status",
            "autostart_stale",
            "list_providers",
            "search_providers",
            "picker_hidden_notice",
            "provider_modes",
            "detect_sources",
            "list_extensions",
            "extension_disclosure",
            "extension_secrets",
            "channels_view",
            "channel_consent",
            "form_schema",
            "visible_fields",
            "field_choices",
            "draft_access",
            "get_account",
            "account_index",
            "vault_audit",
            "doctor_report",
            "backup_preview",
            "preview_session",
            "check_budgets",
            "budget_suggestion",
            "hourly_strip",
            "config_changed_on_disk",
            "updater_status",
            "about_info",
            "whats_new",
            "report_problem",
            "model_state",
            "model_support",
            "load_user_content",
            "core_catalog",
            "locale_registry",
            "get_settings",
            "claude_code_snippets",
            "erase_all_plan",
            "serve_status",
            "hooks_status",
            "widgets_status",
            "cost_status",
            "cost_usage",
            "cost_plans",
            "cost_prices",
            "history_size",
            "account_history",
            "stats",
            "reset_timeline",
        ],
    ),
    (
        REFRESH,
        &[
            "refresh_all",
            "refresh_account",
            "reload_config",
            "load_models",
            "sync_models",
        ],
    ),
    (
        PRESENTATION,
        &[
            "set_tray_prefs",
            "set_advanced_open",
            "set_stats_prefs",
            "set_rows",
            "dismiss_detected",
            "set_whats_new_auto",
            "whats_new_seen",
            "set_account_monogram",
            "set_hidden_accounts",
            "set_tray_menu",
            "set_motion",
            "set_show_pace",
            "set_preference",
            "migrate_local_storage",
            "set_pace_unit",
            "set_advice_enabled",
            "dismiss_feed_banner",
            "cost_set_display",
            "set_locale",
            "set_callback_style",
        ],
    ),
    (
        CALLING_WINDOW,
        &[
            "settings_target",
            "set_settings_title",
            "close_settings_window",
        ],
    ),
    (
        WINDOW_CONTROL,
        &[
            "restart_app",
            "open_settings",
            "show_main_window",
            "quit_app",
        ],
    ),
    (
        EXTERNAL_OPEN,
        &[
            "open_extensions_folder",
            "open_aggregator_dashboard",
            "open_external",
            "open_problem_report",
            "open_user_dir",
        ],
    ),
    (NO_DATA, &["updater_cancel_download", "updater_dismiss"]),
    (MAIN_AND_TRAY_ALERT, &["set_metric_alert"]),
];

/// `Ok` when `label` is one of `allowed`; otherwise the fixed refusal.
pub fn check(label: &str, allowed: &[&str]) -> Result<(), String> {
    if allowed.contains(&label) {
        Ok(())
    } else {
        Err(crate::desk_text::wrong_window())
    }
}

/// [`check`] for the window that invoked the command.
pub fn require<R: Runtime>(window: &WebviewWindow<R>, allowed: &[&str]) -> Result<(), String> {
    check(window.label(), allowed)
}

/// Every name the two tables classify, with repeats.
#[cfg(test)]
pub(crate) fn classified() -> impl Iterator<Item = &'static str> {
    SCOPED
        .iter()
        .flat_map(|(_, _, commands)| commands.iter().copied())
        .chain(
            ANY_WINDOW
                .iter()
                .flat_map(|(_, commands)| commands.iter().copied()),
        )
}

/// The command names of the `tauri::generate_handler![…]` list in `source`
/// (the text of `lib.rs`), with `//` comments and empty entries dropped.
#[cfg(test)]
pub(crate) fn handler_names(source: &str) -> Vec<&str> {
    const OPEN: &str = "tauri::generate_handler![";
    let start = source.find(OPEN).expect("generate_handler! in the source") + OPEN.len();
    let list = &source[start..];
    let list = &list[..list.find(']').expect("the end of generate_handler!")];
    list.lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .flat_map(|line| line.split(','))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect()
}

/// PRD 0050 re-validation S-02: the handler commands of `source` that are not
/// classified exactly once across [`SCOPED`] and [`ANY_WINDOW`].
#[cfg(test)]
pub(crate) fn unclassified(source: &str) -> Vec<&str> {
    handler_names(source)
        .into_iter()
        .filter(|name| classified().filter(|c| c == name).count() != 1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // security-invariant: SEC-WINDOW-SCOPE
    #[test]
    fn a_command_missing_from_the_table_is_reported() {
        let source = "fn run() {\n    builder\n        .invoke_handler(tauri::generate_handler![\n            get_view, // a read\n            wipe_everything,\n            erase_all_data,\n            quit_app\n        ])\n}\n";
        assert_eq!(
            handler_names(source),
            ["get_view", "wipe_everything", "erase_all_data", "quit_app"]
        );
        assert_eq!(unclassified(source), ["wipe_everything"]);
    }

    // security-invariant: SEC-WINDOW-SCOPE
    #[test]
    fn the_tables_use_only_the_named_scopes_and_fixed_reasons() {
        for (name, windows, commands) in SCOPED {
            let expected = match *name {
                "MAIN_ONLY" => MAIN_ONLY,
                "SETTINGS_ONLY" => SETTINGS_ONLY,
                "MAIN_OR_SETTINGS" => MAIN_OR_SETTINGS,
                other => panic!("unknown scope {other}"),
            };
            assert_eq!(*windows, expected, "{name}");
            assert!(!windows.contains(&TRAY), "{name}");
            assert!(!commands.is_empty(), "{name}");
        }
        for (reason, commands) in ANY_WINDOW {
            assert!(REASONS.contains(reason), "{reason}");
            assert!(!commands.is_empty(), "{reason}");
        }
        for reason in REASONS {
            assert!(
                ANY_WINDOW.iter().any(|(r, _)| r == reason),
                "{reason} unused"
            );
        }
    }

    // security-invariant: SEC-WINDOW-SCOPE
    #[test]
    fn each_scope_admits_only_its_windows_and_never_the_tray() {
        for (allowed, admitted) in [
            (MAIN_ONLY, &[MAIN][..]),
            (SETTINGS_ONLY, &[SETTINGS][..]),
            (MAIN_OR_SETTINGS, &[MAIN, SETTINGS][..]),
        ] {
            for label in [MAIN, SETTINGS, TRAY, "", "settings ", "Main"] {
                let result = check(label, allowed);
                assert_eq!(
                    result.is_ok(),
                    admitted.contains(&label),
                    "{label:?} in {allowed:?}"
                );
                if let Err(e) = result {
                    assert!(e.contains("desktop.error.wrong_window"), "{e}");
                }
            }
        }
    }

    // security-invariant: SEC-WINDOW-SCOPE
    #[test]
    fn a_window_that_is_not_allowed_is_refused_by_its_label() {
        use tauri::test::{mock_builder, mock_context, noop_assets};
        use tauri::{WebviewUrl, WebviewWindowBuilder};
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("test app");
        let tray = WebviewWindowBuilder::new(&app, TRAY, WebviewUrl::default())
            .build()
            .expect("tray window");
        let settings = WebviewWindowBuilder::new(&app, SETTINGS, WebviewUrl::default())
            .build()
            .expect("settings window");
        for allowed in [MAIN_ONLY, SETTINGS_ONLY, MAIN_OR_SETTINGS] {
            assert!(require(&tray, allowed).is_err(), "{allowed:?}");
        }
        assert!(require(&settings, SETTINGS_ONLY).is_ok());
        assert!(require(&settings, MAIN_OR_SETTINGS).is_ok());
        assert!(require(&settings, MAIN_ONLY).is_err());
    }
}
