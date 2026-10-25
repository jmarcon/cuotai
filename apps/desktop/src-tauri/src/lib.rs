//! Shell Tauri do Cuotai: janela principal, popover na bandeja e comandos.
//! Somente consome `cuotai-core` (via `backend.rs`); não contém lógica de provider.
mod about;
mod backend;
mod hooks;
mod local_cost;
mod local_server;
mod notify;
mod settings;
mod settings_window;
mod tray_bar;
mod updater;
mod user_content;
mod widgets;

use backend::{
    AccountForm, AccountInput, Backend, FieldChoiceSchema, FormSchema, LoginStartInput,
    ProviderInfo, View,
};
use cuotai_core::{models::ModelState, Provider};
use std::{collections::BTreeMap, sync::Arc};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewWindow, WindowEvent,
};

type Shared<'a> = State<'a, Arc<Backend>>;
/// PRD 0015: the local cost, shared with the local server's feed.
type Cost<'a> = State<'a, Arc<local_cost::LocalCost>>;
/// PRD 0018: the update state, shared by the commands, the 24-hour task and both windows.
type Updates<'a> = State<'a, Arc<updater::Service>>;

#[tauri::command]
async fn get_view(b: Shared<'_>) -> Result<View, String> {
    Ok(b.view().await)
}

#[tauri::command]
async fn refresh_all(app: AppHandle, b: Shared<'_>) -> Result<(), String> {
    b.refresh_all(&app).await;
    Ok(())
}

#[tauri::command]
async fn refresh_account(app: AppHandle, b: Shared<'_>, index: usize) -> Result<(), String> {
    b.refresh_account(&app, index).await
}

#[tauri::command]
fn list_providers() -> Vec<ProviderInfo> {
    backend::providers()
}

/// PRD 0027 RF3.1: the Add-account picker's results, from the core search.
#[tauri::command]
fn search_providers(query: String, locale: String) -> Vec<backend::PickerEntry> {
    backend::search_providers(&query, &locale)
}

/// PRD 0027 RF5.1: every way to connect a provider, recommended first.
#[tauri::command]
fn provider_modes(provider: cuotai_core::Provider) -> Vec<backend::PickerEntry> {
    backend::provider_modes(provider)
}

/// PRD 0027 RF6.2/RF6.5: presence-only detection, run when the picker opens
/// and only after this device agreed to it; dismissed kinds are left out.
#[tauri::command]
async fn detect_sources(b: Shared<'_>) -> Result<Vec<cuotai_core::detect::Detected>, String> {
    let prefs = settings::detect_prefs();
    if prefs.enabled != Some(true) {
        return Ok(Vec::new());
    }
    let configured = b.configured_providers().await;
    let found = tauri::async_runtime::spawn_blocking(move || {
        cuotai_core::detect::local_sources_here(&configured)
    })
    .await
    .map_err(|_| "detect_failed".to_string())?;
    Ok(found
        .into_iter()
        .filter(|d| !prefs.dismissed.iter().any(|k| k == d.kind.slug()))
        .collect())
}

/// PRD 0027 RF6.5: answer (or change) "Check this device…". Error: `unwritable`.
#[tauri::command]
fn set_detect_enabled(on: bool) -> Result<(), String> {
    settings::save_detect_enabled(on).map_err(str::to_owned)
}

/// PRD 0027 RF7.3: "Don't suggest this again". Errors: `invalid_source`, `unwritable`.
#[tauri::command]
fn dismiss_detected(kind: String) -> Result<(), String> {
    settings::dismiss_detected(&kind).map_err(str::to_owned)
}

#[tauri::command]
async fn list_extensions(
    b: Shared<'_>,
) -> Result<Vec<cuotai_core::extensions::management::ExtensionItem>, String> {
    b.extensions().await
}

#[tauri::command]
async fn extension_disclosure(
    b: Shared<'_>,
    id: String,
) -> Result<cuotai_core::extensions::management::Disclosure, String> {
    b.extension_disclosure(&id).await
}

#[tauri::command]
async fn enable_extension(
    app: AppHandle,
    b: Shared<'_>,
    candidate: cuotai_core::extensions::management::Candidate,
) -> Result<cuotai_core::extensions::management::ActionResult, String> {
    b.enable_extension(&app, &candidate).await
}

#[tauri::command]
async fn disable_extension(
    app: AppHandle,
    b: Shared<'_>,
    id: String,
) -> Result<cuotai_core::extensions::management::ActionResult, String> {
    b.disable_extension(&app, &id).await
}

#[tauri::command]
async fn test_extension(
    app: AppHandle,
    b: Shared<'_>,
    id: String,
) -> Result<cuotai_core::extensions::output::Checked, String> {
    b.test_extension(&app, &id).await
}

#[tauri::command]
fn form_schema(provider: Provider) -> FormSchema {
    backend::form_schema(provider)
}

#[tauri::command]
fn visible_fields(
    provider: Provider,
    choices: BTreeMap<String, usize>,
    auth_mode: String,
    login_params: BTreeMap<String, String>,
) -> Vec<&'static str> {
    backend::visible_keys(provider, choices, &auth_mode, &login_params)
}

#[tauri::command]
fn field_choices(
    provider: Provider,
    choices: BTreeMap<String, usize>,
    auth_mode: String,
) -> Vec<FieldChoiceSchema> {
    backend::field_choices(provider, choices, &auth_mode)
}

#[tauri::command]
fn draft_access(
    provider: Provider,
    auth_mode: String,
    fields: Vec<backend::FieldValue>,
    session_slots: Option<Vec<String>>,
) -> backend::AccessView {
    backend::draft_access(
        provider,
        &auth_mode,
        &fields,
        session_slots.as_deref().unwrap_or_default(),
    )
}

#[tauri::command]
async fn login_start(
    app: AppHandle,
    b: Shared<'_>,
    request: LoginStartInput,
) -> Result<u64, String> {
    b.login_start(&app, request).await
}

#[tauri::command]
async fn login_submit_code(b: Shared<'_>, code: String) -> Result<(), String> {
    b.login_submit_code(code).await
}

#[tauri::command]
async fn login_cancel(b: Shared<'_>, attempt: Option<u64>) -> Result<(), String> {
    b.login_cancel(attempt).await;
    Ok(())
}

#[tauri::command]
async fn get_account(b: Shared<'_>, index: usize) -> Result<AccountForm, String> {
    b.get_account(index).await
}

/// PRD 0016 RF1.2: move the account's session variables into the vault,
/// called only after the dialog's explicit confirmation.
#[tauri::command]
async fn migrate_session(
    app: AppHandle,
    b: Shared<'_>,
    index: usize,
) -> Result<AccountForm, String> {
    b.change_session(&app, index, backend::SessionChange::Migrate)
        .await
}

/// PRD 0016 RF4.2: forget the account's web session (its vault entries are
/// deleted), called only after the dialog's explicit confirmation.
#[tauri::command]
async fn forget_session(
    app: AppHandle,
    b: Shared<'_>,
    index: usize,
) -> Result<AccountForm, String> {
    b.change_session(&app, index, backend::SessionChange::Forget)
        .await
}

/// PRD 0029 RF6.2: the vault entries Cuotai created; never a value.
#[tauri::command]
async fn vault_audit(
    b: Shared<'_>,
) -> Result<Vec<cuotai_core::oauth::vault_index::AuditEntry>, String> {
    b.vault_audit().await
}

/// PRD 0029 RF6.2: delete the vault entries no account uses.
#[tauri::command]
async fn vault_prune(b: Shared<'_>) -> Result<usize, String> {
    b.vault_prune().await
}

/// PRD 0016 RF2.2: what a paste keeps (names, counts, expiry), before it is
/// saved; never the value.
#[tauri::command]
fn preview_session(
    provider: Provider,
    slot: String,
    paste: String,
) -> Result<backend::SessionPreview, String> {
    backend::preview_session(provider, &slot, paste)
}

/// PRD 0036 RF8.2: "Open dashboard" of the aggregator account at `index`
/// (its instance's usage page, in the browser). Errors: `not_found`,
/// `not_listed`, `unavailable`.
#[tauri::command]
async fn open_aggregator_dashboard(b: Shared<'_>, index: usize) -> Result<(), String> {
    let url = b.aggregator_dashboard(index).await.ok_or("not_found")?;
    about::open_dashboard(&url).map_err(str::to_owned)
}

/// PRD 0036 RF2.2: the account dialog's Test connection of an aggregator.
#[tauri::command]
async fn test_aggregator(
    b: Shared<'_>,
    input: AccountInput,
) -> Result<backend::AggregatorTestView, String> {
    b.test_aggregator(input).await
}

/// PRD 0016 RF2.1: one read-only query of the dialog's draft with its pasted
/// sessions; nothing is stored.
#[tauri::command]
async fn test_session(
    b: Shared<'_>,
    input: AccountInput,
) -> Result<backend::SessionTestView, String> {
    b.test_session(input).await
}

#[tauri::command]
async fn save_account(
    app: AppHandle,
    b: Shared<'_>,
    input: AccountInput,
) -> Result<backend::SaveAccountResult, String> {
    b.save_account(&app, input).await
}

#[tauri::command]
async fn delete_account(
    app: AppHandle,
    b: Shared<'_>,
    index: usize,
    purge: bool,
) -> Result<backend::DeleteResult, String> {
    b.delete_account(&app, index, purge).await
}

/// RF3: retry a vault purge using the opaque handle the previous
/// `delete_account` returned. The retry runs against the *current*
/// configuration, so the core's shared-key guard naturally decides
/// whether the credential is still shared (refused with an honest
/// message) or unique (purged). The handle is consumed on success.
#[tauri::command]
async fn retry_purge(
    app: AppHandle,
    b: Shared<'_>,
    handle: String,
) -> Result<backend::DeleteResult, String> {
    b.retry_purge(&app, handle).await
}

#[tauri::command]
async fn set_refresh_seconds(app: AppHandle, b: Shared<'_>, seconds: u64) -> Result<(), String> {
    b.set_refresh_seconds(&app, seconds).await
}

#[tauri::command]
async fn move_account(app: AppHandle, b: Shared<'_>, from: usize, to: usize) -> Result<(), String> {
    b.move_account(&app, from, to).await
}

/// RF4: re-read the config from disk and rebuild the live state
/// (monitor/cycle/tokens/RF3 recovery). Used after `EditError::Conflict`
/// and to pick up external edits detected via [`config_changed_on_disk`].
/// Returns the new view so the frontend can render without a second
/// round-trip.
#[tauri::command]
async fn reload_config(app: AppHandle, b: Shared<'_>) -> Result<backend::View, String> {
    b.reload_config(&app).await
}

/// RF4: returns `true` when the file on disk differs from the
/// snapshot the backend last loaded/saved. Cheap (byte comparison,
/// not mtime) so the UI can poll on window focus / before opening
/// the editor.
#[tauri::command]
async fn config_changed_on_disk(b: Shared<'_>) -> Result<bool, String> {
    b.config_changed_on_disk().await
}

/// PRD 0002 RF6.1: model catalog of account `index`, guarded by the
/// `generation` of the `View` the UI read. A result for another generation
/// fails with `"stale"` and must be discarded. No secret crosses the IPC.
#[tauri::command]
async fn model_state(b: Shared<'_>, index: usize, generation: u64) -> Result<ModelState, String> {
    b.models(index, generation, |m, i| async move { m.model_state(i) })
        .await
}

/// PRD 0028 RF2.3: the secret-free problem report of account `index`, or of the
/// application alone when `index` is absent (About). Nothing is sent.
#[tauri::command]
async fn report_problem(
    b: Shared<'_>,
    index: Option<usize>,
    generation: u64,
) -> Result<backend::ProblemDraft, String> {
    b.report_problem(index, generation).await
}

/// First explicit opening: queries only when nothing is recorded.
#[tauri::command]
async fn load_models(b: Shared<'_>, index: usize, generation: u64) -> Result<ModelState, String> {
    b.models(
        index,
        generation,
        |m, i| async move { m.load_models(i).await },
    )
    .await
}

/// Manual "Sincronizar modelos".
#[tauri::command]
async fn sync_models(b: Shared<'_>, index: usize, generation: u64) -> Result<ModelState, String> {
    b.models(
        index,
        generation,
        |m, i| async move { m.sync_models(i).await },
    )
    .await
}

/// Catalog source or unsupported reason of `provider` for a new account.
#[tauri::command]
fn model_support(provider: Provider) -> backend::ModelSupport {
    backend::model_support(provider)
}

#[tauri::command]
fn load_user_content() -> Result<user_content::UserContent, String> {
    user_content::load().ok_or_else(|| "Pasta de configuração indisponível.".to_string())
}

/// RF8: ship the core-owned locale catalog (`crates/cuotai-core/locales/*.json`)
/// to the desktop frontend so it can resolve every id the core emits without
/// shipping those strings itself. `locale` follows the same precedence as
/// `cuotai_core::text::Locale::resolve`; pt-* returns an empty catalog so
/// the frontend falls back to the Portuguese `fallback` text embedded in
/// each `Localized`.
#[tauri::command]
fn core_catalog(locale: String) -> std::collections::BTreeMap<String, String> {
    cuotai_core::text::core_catalog(&locale)
        .map(|catalog| {
            catalog
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// Linhas do menu do ícone da bandeja (clique direito), já traduzidas pela interface: o shell não conhece idioma.
#[derive(serde::Deserialize)]
struct TrayMenuPayload {
    /// Uma linha por conta: "nome: % disponível".
    lines: Vec<String>,
    /// Texto quando não há contas.
    empty: String,
    open: String,
    settings: String,
    quit: String,
    /// PRD 0010 RF3.3: icon tooltip ("Cuotai · oldest reading: 42 min ago").
    tooltip: String,
    /// PRD 0003 RF3.4: "Next notice: Claude 5h · today, 14:30", when one is ahead.
    #[serde(default)]
    notice: Option<String>,
    /// PRD 0018 RF2.2: "Update 0.72.0 available" at the top of the menu; clicking opens
    /// the main window, where the notice lives.
    #[serde(default)]
    update: Option<String>,
}

/// Remove controles, apara e limita o tamanho de uma linha de menu.
fn clean_menu_line(text: &str) -> String {
    let cleaned: String = text.chars().filter(|c| !c.is_control()).collect();
    let trimmed = cleaned.trim();
    if trimmed.chars().count() <= 80 {
        trimmed.to_owned()
    } else {
        format!("{}…", trimmed.chars().take(79).collect::<String>())
    }
}

fn build_tray_menu(app: &AppHandle, p: &TrayMenuPayload) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    // PRD 0018 RF2.2: a atualização vem antes de tudo; o resto do menu é o de sempre.
    if let Some(update) = p
        .update
        .as_deref()
        .map(clean_menu_line)
        .filter(|u| !u.is_empty())
    {
        menu.append(&MenuItem::with_id(
            app,
            "update",
            update,
            true,
            None::<&str>,
        )?)?;
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    if let Some(notice) = p
        .notice
        .as_deref()
        .map(clean_menu_line)
        .filter(|n| !n.is_empty())
    {
        menu.append(&MenuItem::with_id(
            app,
            "notice",
            notice,
            false,
            None::<&str>,
        )?)?;
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    let lines: Vec<String> = p
        .lines
        .iter()
        .map(|l| clean_menu_line(l))
        .filter(|l| !l.is_empty())
        .take(40)
        .collect();
    if lines.is_empty() {
        menu.append(&MenuItem::with_id(
            app,
            "none",
            clean_menu_line(&p.empty),
            false,
            None::<&str>,
        )?)?;
    }
    // Habilitadas (texto com contraste normal); clicar numa conta abre a janela principal.
    for line in &lines {
        menu.append(&MenuItem::with_id(
            app,
            "account",
            line,
            true,
            None::<&str>,
        )?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "open",
        clean_menu_line(&p.open),
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "settings",
        clean_menu_line(&p.settings),
        true,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "quit",
        clean_menu_line(&p.quit),
        true,
        None::<&str>,
    )?)?;
    Ok(menu)
}

/// Atualiza o menu do clique direito. Comando síncrono: roda na thread principal, onde o menu nativo vive.
#[tauri::command]
fn set_tray_menu(app: AppHandle, payload: TrayMenuPayload) -> Result<(), String> {
    // PRD 0020 RF1.4: the localized "Settings" also titles the next Settings
    // window and the macOS app menu item.
    app.state::<settings_window::State>()
        .set_title(&payload.settings);
    #[cfg(target_os = "macos")]
    if let Some(item) = app.try_state::<AppMenuSettings>() {
        let _ = item
            .0
            .set_text(format!("{}…", clean_menu_line(&payload.settings)));
    }
    let menu = build_tray_menu(&app, &payload).map_err(|e| e.to_string())?;
    if let Some(tray) = app.tray_by_id("cuotai") {
        tray.set_menu(Some(menu)).map_err(|e| e.to_string())?;
    }
    // PRD 0025 RF7.1: the summary's label first, then these lines.
    tray_bar::set_menu_tooltip(&app, tray_tooltip(&payload));
    Ok(())
}

#[tauri::command]
fn about_info() -> about::AboutView {
    about::view()
}

/// PRD 0019 RF7.7: What's new, from the changelog embedded in the binary (no network).
#[tauri::command]
fn whats_new() -> about::WhatsNewView {
    about::whats_new()
}

/// Abre no navegador um repositório da lista do "Sobre" ou uma página pública de status de uma fonte
/// verificada (PRD 0011 RF4.1); qualquer outro endereço é recusado.
#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    about::open_external(&url).map_err(str::to_owned)
}

#[tauri::command]
fn get_settings() -> settings::SettingsView {
    settings::view()
}

/// `path = None` volta à pasta padrão. Os erros são códigos estáveis que a UI traduz.
#[tauri::command]
fn set_user_dir(app: AppHandle, path: Option<String>) -> Result<(), String> {
    if settings::env_user_dir().is_some() {
        return Err("env_locked".into());
    }
    let dir = path.map(std::path::PathBuf::from);
    settings::save_user_dir(dir.as_deref()).map_err(str::to_owned)?;
    let _ = app.emit("user-content-changed", ());
    Ok(())
}

#[tauri::command]
fn open_user_dir() -> Result<(), String> {
    settings::open_user_dir().map_err(str::to_owned)
}

/// PRD 0004 RF1.2: `system`, `always`, `reduced` ou `off`. Erros: `invalid_motion`, `unwritable`.
#[tauri::command]
fn set_motion(motion: String) -> Result<(), String> {
    settings::save_motion(&motion).map_err(str::to_owned)
}

/// PRD 0020 RF7.2: one of `theme`, `theme_mode`, `sort`, `group_recharging`, saved in
/// `desktop.json`, the single source; every window hears the `settings-changed` event
/// (RF7.1) and re-reads. Errors: `invalid_preference`, `unwritable`.
#[tauri::command]
fn set_preference(app: AppHandle, key: String, value: serde_json::Value) -> Result<(), String> {
    settings::save_preference(&key, &value).map_err(str::to_owned)?;
    let _ = app.emit("settings-changed", serde_json::json!({ "key": key }));
    Ok(())
}

/// PRD 0020 RF7.2: the one-time copy of the old `localStorage` values (keys `theme`,
/// `theme_mode`, `sort`, `group_recharging`, `motion`, `locale`); returns the settings.
#[tauri::command]
fn migrate_local_storage(
    app: AppHandle,
    values: BTreeMap<String, String>,
) -> Result<settings::SettingsView, String> {
    if settings::migrate_local_storage(&values).map_err(str::to_owned)? {
        let _ = app.emit("settings-changed", serde_json::json!({ "key": "" }));
    }
    Ok(settings::view())
}

/// PRD 0025 RF4.1: the menu-bar summary `style` and `accounts` (`None` =
/// automatic) would produce from the current readings, for the Settings
/// preview. Error: `invalid_tray_style`.
#[tauri::command]
fn tray_preview(
    style: String,
    accounts: Option<Vec<serde_json::Value>>,
) -> Result<cuotai_core::tray::TraySummary, String> {
    let style = cuotai_core::tray::TrayStyle::parse(&style).ok_or("invalid_tray_style")?;
    let selection = match accounts {
        None => cuotai_core::tray::TraySelection::Auto,
        Some(ids) => cuotai_core::tray::TraySelection::Custom(settings::clean_ids(&ids)),
    };
    Ok(tray_bar::preview(style, &selection))
}

/// PRD 0025 RF4.2: the accounts the Settings list offers, dashboard order.
#[tauri::command]
fn tray_accounts() -> Vec<cuotai_core::tray::AccountView> {
    tray_bar::accounts()
}

/// PRD 0025 RF2.2/RF3.6/RF4.3: save the menu-bar choices, redraw the item
/// and tell every window (the popover follows the selection and the view).
/// `accounts` `None` is automatic; ids of removed accounts are dropped.
/// Errors: `invalid_tray_style`, `invalid_popover_view`, `unwritable`.
#[tauri::command]
fn set_tray_prefs(
    app: AppHandle,
    style: String,
    colored: bool,
    accounts: Option<Vec<serde_json::Value>>,
    popover_view: Option<String>,
) -> Result<(), String> {
    let style = cuotai_core::tray::TrayStyle::parse(&style).ok_or("invalid_tray_style")?;
    let popover_view = popover_view
        .map(|v| serde_json::from_value(serde_json::Value::String(v)).map_err(|_| "invalid_popover_view"))
        .transpose()?;
    let existing = tray_bar::account_keys();
    let accounts = accounts.map(|ids| {
        let ids = settings::clean_ids(&ids);
        if existing.is_empty() {
            ids
        } else {
            ids.into_iter().filter(|id| existing.contains(id)).collect()
        }
    });
    let prefs = settings::TrayPrefs {
        style,
        colored,
        accounts,
        popover_view,
    };
    settings::save_tray(&prefs).map_err(str::to_owned)?;
    tray_bar::redraw(&app, &prefs);
    let _ = app.emit("tray-prefs-changed", backend::tray_view());
    Ok(())
}

/// PRD 0025 RF5.3: remember whether the Settings sidebar's Advanced group is
/// open. Error: `unwritable`.
#[tauri::command]
fn set_advanced_open(open: bool) -> Result<(), String> {
    let file = settings::file().map_err(str::to_owned)?;
    settings::write_advanced_open(&file, open).map_err(str::to_owned)
}

/// PRD 0025 RF6.1/RF6.2/RF6.6: **Turn on** of the first run. Asks the
/// operating system for permission once and, when granted, turns usage
/// alerts on with the core's recommended thresholds, exhaustion and recovery
/// (`AlertPrefs::recommended`); nothing else changes (no status pages,
/// local cost, server, hooks, widgets or update checks). Records the step as
/// seen either way. Returns the permission. Error: `unwritable`.
#[tauri::command]
async fn first_run_turn_on(app: AppHandle, b: Shared<'_>) -> Result<String, String> {
    let permission = notify::request_permission(&app);
    let recommended = cuotai_core::alerts::AlertPrefs::recommended();
    let thresholds: Vec<u8> = recommended.thresholds().collect();
    if permission == "granted" {
        b.update_notices(&app, |n| {
            let mut alerts =
                n.alerts()
                    .with_choices(&thresholds, recommended.exhaustion, recommended.recovery)?;
            // PRD 0023 RF4.6: the extra-usage notices are part of the recommendation.
            alerts.overage = true;
            n.set_alerts(alerts)?;
            n.switch_alerts(permission)
        })
        .await?;
    }
    record_first_run(Some(settings::ONBOARDING_VERSION))?;
    Ok(permission.to_owned())
}

/// PRD 0025 RF6.1/RF6.4: **Not now** records the step only; `again` (Settings
/// › General, "Show the recommended setup again") clears the record.
/// Error: `unwritable`.
#[tauri::command]
fn first_run_record(again: bool) -> Result<(), String> {
    record_first_run((!again).then_some(settings::ONBOARDING_VERSION))
}

fn record_first_run(version: Option<u64>) -> Result<(), String> {
    let file = settings::file().map_err(str::to_owned)?;
    settings::write_onboarding(&file, version).map_err(str::to_owned)
}

/// PRD 0007 RF3.4: show or hide the consumption pace. Error: `unwritable`.
#[tauri::command]
fn set_show_pace(show: bool) -> Result<(), String> {
    settings::save_show_pace(show).map_err(str::to_owned)
}

/// PRD 0006 RF4.1: days of measurement history kept on this device (7 to
/// 365), saved in `desktop.json`; prunes at once. Errors:
/// `invalid_history_days`, `unwritable`.
#[tauri::command]
async fn set_history_days(app: AppHandle, b: Shared<'_>, days: u32) -> Result<(), String> {
    b.set_history_days(&app, days).await
}

/// PRD 0006 RF4.4: export the history of account `index` (`None` = every
/// account) of the last `days` days (`None` = everything) to the file the
/// user chose in the native save dialog. Returns the rows written. An account
/// `index` comes with the configuration `generation` it was read from (any
/// other one: `stale`).
#[tauri::command]
async fn history_export(
    b: Shared<'_>,
    index: Option<usize>,
    generation: Option<u64>,
    days: Option<u32>,
    format: cuotai_core::history::ExportFormat,
    path: String,
) -> Result<usize, String> {
    b.history_export(
        index,
        generation,
        days,
        format,
        std::path::PathBuf::from(path),
    )
    .await
}

/// PRD 0006 RF4.4: erase the history of account `index`, or all of it with
/// `None`, after the confirmation the UI asked for. Returns the samples removed.
/// An account `index` comes with its configuration `generation` (any other
/// one: `stale`).
#[tauri::command]
async fn history_erase(
    app: AppHandle,
    b: Shared<'_>,
    index: Option<usize>,
    generation: Option<u64>,
) -> Result<usize, String> {
    b.history_erase(&app, index, generation).await
}

/// PRD 0006 RF5.3: record new readings on this device or not (saved in
/// `desktop.json`); off keeps what was recorded. Error: `unwritable`.
#[tauri::command]
async fn set_history_recording(app: AppHandle, b: Shared<'_>, on: bool) -> Result<(), String> {
    b.set_history_recording(&app, on).await
}

/// PRD 0011 RF4.3: turn the public provider status queries on or off
/// (saved in `desktop.json`, off by default). Error: `unwritable`.
#[tauri::command]
async fn set_status_enabled(app: AppHandle, b: Shared<'_>, on: bool) -> Result<(), String> {
    b.set_status_enabled(&app, on).await
}

/// PRD 0012 RF5.2: the local server's state and choices (off, on at its
/// address, or why it is not serving).
#[tauri::command]
async fn serve_status(b: Shared<'_>) -> Result<local_server::ServeView, String> {
    Ok(b.serve_view().await)
}

/// PRD 0012 RF5.1: turn the local server on or off (off by default), its
/// port (bound exactly) and "allow browsers", saved in `desktop.json`; the
/// server restarts with them. Errors: `invalid_port`, `unwritable`.
#[tauri::command]
async fn set_serve(
    app: AppHandle,
    b: Shared<'_>,
    enabled: bool,
    port: u16,
    allow_browsers: bool,
) -> Result<local_server::ServeView, String> {
    b.set_serve(&app, enabled, port, allow_browsers).await
}

/// PRD 0012 RF5.1: generate the server's token into the OS vault (a new
/// one invalidates the previous) and require it.
#[tauri::command]
async fn generate_serve_token(
    app: AppHandle,
    b: Shared<'_>,
) -> Result<local_server::ServeView, String> {
    b.generate_serve_token(&app).await
}

/// PRD 0012 RF5.1: revoke the server's token (removed from the vault).
#[tauri::command]
async fn revoke_serve_token(
    app: AppHandle,
    b: Shared<'_>,
) -> Result<local_server::ServeView, String> {
    b.revoke_serve_token(&app).await
}

/// PRD 0012 RF5.1: the token, only for the "Copy token" button.
#[tauri::command]
async fn copy_serve_token(b: Shared<'_>) -> Result<String, String> {
    b.copy_serve_token().await
}

/// PRD 0013 RF4.1: the hooks in Settings → Integration: the switch, the
/// state, the rules of `hooks.toml` for the editor, the listing awaiting
/// confirmation and the latest runs.
#[tauri::command]
async fn hooks_status(app: AppHandle, b: Shared<'_>) -> Result<hooks::HooksView, String> {
    Ok(b.hooks_view(&app).await)
}

/// PRD 0014 RF4.1: the widget snapshot as Settings shows it: the switch,
/// whether identity travels with it, how many accounts are published, the
/// file location and whether it is the *app group* an extension can read.
#[tauri::command]
async fn widgets_status(b: Shared<'_>) -> Result<widgets::WidgetsView, String> {
    Ok(b.widgets_view().await)
}

/// PRD 0014 RF4.1: publish the snapshot (off by default, saved in
/// `desktop.json`) or stop and delete the file; `include_identity` and
/// `max_accounts` travel with the switch. Errors:
/// `invalid_widget_accounts`, `unwritable`.
#[tauri::command]
async fn set_widgets(
    app: AppHandle,
    b: Shared<'_>,
    enabled: bool,
    include_identity: bool,
    max_accounts: usize,
) -> Result<widgets::WidgetsView, String> {
    b.set_widgets(&app, enabled, include_identity, max_accounts)
        .await
}

/// PRD 0014 RF4.1: "Delete widget data" — removes `widget.json` now.
#[tauri::command]
async fn delete_widget_data(app: AppHandle, b: Shared<'_>) -> Result<(), String> {
    b.delete_widget_data(&app).await
}

/// PRD 0013 RF3.2/RF4: turn the hooks on or off (off by default, saved in
/// `desktop.json`); on runs only confirmed rules. Error: `unwritable`.
#[tauri::command]
async fn set_hooks_enabled(
    app: AppHandle,
    b: Shared<'_>,
    on: bool,
) -> Result<hooks::HooksView, String> {
    b.set_hooks_enabled(&app, on).await
}

/// PRD 0013 RF4.2: the user confirmed the exact commands the dialog listed
/// under `digest`. Error: the core's `hooks.error.*` text as JSON.
#[tauri::command]
async fn confirm_hooks(
    app: AppHandle,
    b: Shared<'_>,
    digest: String,
) -> Result<hooks::HooksView, String> {
    b.confirm_hooks(&app, digest).await
}

/// PRD 0013 RF4.1: save the editor's rules into `hooks.toml` over the
/// `revision` it read. Errors: `no_config`, the core's `hooks.error.*`
/// text as JSON.
#[tauri::command]
async fn save_hooks(
    app: AppHandle,
    b: Shared<'_>,
    revision: Option<String>,
    rules: Vec<cuotai_core::hooks::RuleDraft>,
) -> Result<hooks::HooksView, String> {
    b.save_hooks(&app, revision, rules).await
}

/// PRD 0013 RF4.1: fire a sample event at the confirmed rule `name`.
/// Errors: `hooks_off`, `hook_unknown`.
#[tauri::command]
async fn test_hook(
    app: AppHandle,
    b: Shared<'_>,
    name: String,
) -> Result<hooks::HooksView, String> {
    b.test_hook(&app, name).await
}

/// PRD 0003 RF3.3: turn reset notices on or off. On asks the operating system
/// for permission first and stays off unless it is `granted`; returns the
/// permission (`granted`, `denied`, `prompt`) or `off`. Error: `unwritable`.
#[tauri::command]
async fn set_notify_enabled(app: AppHandle, b: Shared<'_>, on: bool) -> Result<String, String> {
    let permission = permission_for(&app, on);
    b.update_notices(&app, |n| n.switch_resets(permission))
        .await?;
    Ok(permission.to_owned())
}

/// PRD 0008 RF3.1/RF3.3: turn usage alerts on or off, with the same
/// permission request as reset notices: on stays off unless the system
/// answers `granted`. Returns the permission or `off`. Error: `unwritable`.
#[tauri::command]
async fn set_alerts_enabled(app: AppHandle, b: Shared<'_>, on: bool) -> Result<String, String> {
    let permission = permission_for(&app, on);
    b.update_notices(&app, |n| n.switch_alerts(permission))
        .await?;
    Ok(permission.to_owned())
}

/// Turning a notice switch on asks the operating system first.
fn permission_for(app: &AppHandle, on: bool) -> &'static str {
    if on {
        notify::request_permission(app)
    } else {
        "off"
    }
}

/// PRD 0008 RF3.1: the alert thresholds (each 10 to 99%) and the
/// exhaustion and recovery notices, saved in `desktop.json`. Errors:
/// `invalid_threshold`, `unwritable`.
#[tauri::command]
async fn set_alert_options(
    app: AppHandle,
    b: Shared<'_>,
    thresholds: Vec<u8>,
    exhaustion: bool,
    recovery: bool,
    overage: Option<bool>,
) -> Result<(), String> {
    b.update_notices(&app, |n| {
        let mut alerts = n.alerts().with_choices(&thresholds, exhaustion, recovery)?;
        // PRD 0023 RF7.4: the extra-usage notices (absent keeps the saved value).
        if let Some(overage) = overage {
            alerts.overage = overage;
        }
        n.set_alerts(alerts)
    })
    .await
}

/// PRD 0008 RF3.2: mute (`on = false`) or unmute the usage alerts of the
/// metric `label` of the account with the core key `account_key`. Error:
/// `unwritable`.
#[tauri::command]
async fn set_metric_alert(
    app: AppHandle,
    b: Shared<'_>,
    account_key: String,
    label: String,
    on: bool,
) -> Result<(), String> {
    b.update_notices(&app, |n| n.set_metric_alert(&account_key, &label, on))
        .await
}

/// PRD 0036 RF6.4: whether the aggregator account `account_key` alerts on
/// each connection (`each = true`) or only when a whole pool runs out.
/// Error: `unwritable`.
#[tauri::command]
async fn set_pool_alerts(
    app: AppHandle,
    b: Shared<'_>,
    account_key: String,
    each: bool,
) -> Result<(), String> {
    b.update_notices(&app, |n| n.set_pool_alerts(&account_key, each))
        .await
}

/// PRD 0003 RF3.1: lead time (0, 5, 15 or 60 min), filter (`used`,
/// `exhausted`, `always`) and "other" windows, saved in `desktop.json`.
/// Errors: `invalid_notify`, `unwritable`.
#[tauri::command]
async fn set_notify_options(
    app: AppHandle,
    b: Shared<'_>,
    lead_minutes: u32,
    when: String,
    include_other: bool,
) -> Result<(), String> {
    b.update_notices(&app, |n| {
        let prefs = n.prefs().with_choices(lead_minutes, &when, include_other)?;
        n.set_prefs(prefs)
    })
    .await
}

/// PRD 0003 RF3.2: the bell of the account with the core key `account_key`.
/// Error: `unwritable`.
#[tauri::command]
async fn set_account_notify(
    app: AppHandle,
    b: Shared<'_>,
    account_key: String,
    on: bool,
) -> Result<(), String> {
    b.update_notices(&app, |n| n.set_account(&account_key, on))
        .await
}

/// PRD 0003 RF2.2: the language chosen in the app, for the notices the
/// backend sends. Errors: `invalid_locale`, `unwritable`.
#[tauri::command]
async fn set_locale(app: AppHandle, b: Shared<'_>, locale: String) -> Result<(), String> {
    let differs = settings::settings_path().is_none_or(|f| settings::locale_differs(&f, &locale));
    b.update_notices(&app, |n| n.set_locale(&locale)).await?;
    if differs {
        let _ = app.emit("settings-changed", serde_json::json!({ "key": "locale" }));
    }
    Ok(())
}

/// The browser page after a sign-in callback follows the interface language
/// and the active theme. `colors` holds `#rgb`/`#rrggbb` values for `bg`,
/// `surface`, `border`, `fg`, `fg_muted` and `bad`; without it (or when one
/// is invalid) the page keeps the browser's own light/dark preference.
#[tauri::command]
fn set_callback_style(
    locale: String,
    scheme: String,
    colors: Option<std::collections::HashMap<String, String>>,
) {
    use cuotai_core::oauth::callback_page::{
        set_callback_style, CallbackPalette, CallbackScheme, CallbackStyle,
    };
    let scheme = match scheme.as_str() {
        "dark" => Some(CallbackScheme::Dark),
        "light" => Some(CallbackScheme::Light),
        _ => None,
    };
    let theme = scheme.zip(colors).and_then(|(scheme, c)| {
        let get = |k: &str| c.get(k).cloned();
        Some((
            scheme,
            CallbackPalette {
                bg: get("bg")?,
                surface: get("surface")?,
                border: get("border")?,
                fg: get("fg")?,
                fg_muted: get("fg_muted")?,
                bad: get("bad")?,
            },
        ))
    });
    let style = CallbackStyle {
        locale: locale.clone(),
        theme,
    };
    if !set_callback_style(style) {
        // An invalid palette must not lose the language.
        set_callback_style(CallbackStyle {
            locale,
            theme: None,
        });
    }
}

/// PRD 0003 RF3.3: send one notice now, with the texts the interface
/// translated, to check the permission and the notification center. Only
/// while reset notices or usage alerts are on: on desktop the first notice
/// is what makes the system decide the permission. Error: `notify_off`.
#[tauri::command]
async fn send_test_notification(
    app: AppHandle,
    b: Shared<'_>,
    title: String,
    body: String,
) -> Result<(), String> {
    use notify::Notifier;
    if !b.notices_enabled().await {
        return Err("notify_off".into());
    }
    notify::SystemNotifier(app).deliver(&clean_menu_line(&title), &clean_menu_line(&body))
}

/// PRD 0015 RF1/RF2/RF4.1: Settings → Local cost: each source with its
/// disclosure, chosen directories, what the index follows and the last
/// scan, the period and the retention.
#[tauri::command]
async fn cost_status(cost: Cost<'_>) -> Result<local_cost::CostView, String> {
    cost.view()
}

/// PRD 0015 RF4.1: the cards' and details' estimate (the core's
/// `CostSummary`): per source today, yesterday and the saved period, by day
/// and by model, and why a source has no numbers. Reads no log.
#[tauri::command]
async fn cost_usage(cost: Cost<'_>) -> Result<cuotai_core::cost::CostSummary, String> {
    cost.usage()
}

/// PRD 0015 RF1.1: turn `source` on after the user accepted the
/// disclosure `digest` the dialog showed. Error: the core's
/// `cost.error.*` text as JSON (`consent_changed` when the directories
/// changed since).
#[tauri::command]
async fn cost_enable(
    cost: Cost<'_>,
    source: cuotai_core::cost::SourceId,
    digest: String,
) -> Result<local_cost::CostView, String> {
    cost.enable(source, &digest)
}

/// PRD 0015 RF2.4: turn `source` off and erase everything derived from it.
#[tauri::command]
async fn cost_disable(
    cost: Cost<'_>,
    source: cuotai_core::cost::SourceId,
) -> Result<local_cost::CostView, String> {
    cost.disable(source)
}

/// PRD 0015 RF4.1: the directories `source` reads (none: the defaults). A
/// different list turns a source that is on off and erases what was
/// derived from it. Error: `cost.error.relative_path` as JSON.
#[tauri::command]
async fn cost_set_paths(
    cost: Cost<'_>,
    source: cuotai_core::cost::SourceId,
    paths: Vec<String>,
) -> Result<local_cost::CostView, String> {
    cost.set_paths(source, paths)
}

/// PRD 0015 RF2.3: the period shown and the days the index keeps (7 to
/// 730). Error: `cost.error.retention_bounds` as JSON.
#[tauri::command]
async fn cost_set_options(
    cost: Cost<'_>,
    period: cuotai_core::cost::Period,
    retention_days: u32,
) -> Result<local_cost::CostView, String> {
    cost.set_options(period, retention_days)
}

/// PRD 0015 RF4.1: the prices in effect (the embedded table under the
/// user's `pricing.json`) for Settings → Local cost.
#[tauri::command]
async fn cost_prices(cost: Cost<'_>) -> Result<local_cost::PricesView, String> {
    cost.prices()
}

/// PRD 0015 RF2.4: erase the whole cost index after the confirmation the
/// UI asked for. Returns the rows removed.
#[tauri::command]
async fn cost_erase(cost: Cost<'_>) -> Result<usize, String> {
    cost.erase()
}

/// PRD 0006 RF5.3: bytes the history file takes on this device.
#[tauri::command]
async fn history_size(b: Shared<'_>) -> Result<u64, String> {
    b.history_size().await
}

/// PRD 0006 RF5.1: the recorded history of account `index` over `range`
/// (`day`, `week`, `month`), every metric or only `metric_key`, as the core
/// computed it, guarded by the `generation` of the `View` the UI read like
/// the model catalog: a result for another generation fails with `"stale"`
/// and must be discarded. No secret crosses the IPC.
#[tauri::command]
async fn account_history(
    b: Shared<'_>,
    index: usize,
    generation: u64,
    range: cuotai_core::history::HistoryRange,
    metric_key: Option<String>,
) -> Result<backend::HistoryView, String> {
    b.account_history(index, generation, range, metric_key)
        .await
}

/// PRD 0026 RF4.6: the usage statistics over `range` (`days7`, `days30`) of
/// one account (`account`) or all (`null`) and the recent journal events, as
/// the core computed them from the stored history. Guarded by the
/// `generation` of the `View` the UI read: another generation fails with
/// `"stale"` and must be discarded. No secret crosses the IPC.
#[tauri::command]
async fn stats(
    b: Shared<'_>,
    range: cuotai_core::stats::StatsRange,
    account: Option<usize>,
    generation: u64,
) -> Result<backend::StatsView, String> {
    b.stats(range, account, generation).await
}

/// PRD 0026 RF5: the reset timeline (`hours24`, `days7`) of the readings the
/// app holds, guarded by `generation` like `stats`.
#[tauri::command]
async fn reset_timeline(
    b: Shared<'_>,
    range: cuotai_core::resets::TimelineRange,
    generation: u64,
) -> Result<cuotai_core::resets::Timeline, String> {
    b.reset_timeline(range, generation).await
}

/// RF1.2: `path = None` volta ao `cuotai.toml` padrão do SO. Recarrega pelo core e só então
/// grava a escolha em `desktop.json`. Erros: códigos de `settings` ou o JSON tipado do core.
#[tauri::command]
async fn set_config_file(
    app: AppHandle,
    b: Shared<'_>,
    path: Option<String>,
) -> Result<View, String> {
    b.set_config_file(&app, path.map(std::path::PathBuf::from))
        .await
}

/// PRD 0018 RF3.1: o estado que Settings → Updates e "Sobre" mostram: o interruptor, a
/// última verificação, a versão atual, o canal, o motivo de estar desligado e a fase
/// (nada de URL, assinatura ou chave).
#[tauri::command]
fn updater_status(u: Updates<'_>) -> updater::UpdateView {
    u.view()
}

/// PRD 0018 RF2.6/RF3.2: enables or disables automatic checks in `desktop.json`
/// after the one-time answer. Off sends no request. Errors:
/// `updater_consent_required`, `unwritable`.
#[tauri::command]
fn updater_set_automatic(
    app: AppHandle,
    u: Updates<'_>,
    on: bool,
) -> Result<updater::UpdateView, String> {
    u.set_automatic(&app, on)
}

/// PRD 0018 RF2.6: records the one-time answer; "Yes" enables the schedule and
/// "No" leaves it off. The backend opens its request gate only after persistence.
#[tauri::command]
fn updater_answer_automatic(
    app: AppHandle,
    u: Updates<'_>,
    on: bool,
) -> Result<updater::UpdateView, String> {
    u.answer_automatic(&app, on)
}

/// PRD 0018 RF3.1: "Verificar agora". Com o recurso desligado responde `updater_off` e
/// não faz pedido algum.
#[tauri::command]
async fn updater_check_now(app: AppHandle, u: Updates<'_>) -> Result<updater::UpdateView, String> {
    u.check_now(&app).await
}

/// PRD 0018 RF2.3: "Atualizar agora" — baixa, verifica a assinatura e instala. Erros:
/// `no_update`, `already_running`, `stale`, `unavailable`.
#[tauri::command]
fn updater_install(app: AppHandle, u: Updates<'_>) -> Result<updater::UpdateView, String> {
    u.install(&app)
}

/// PRD 0018 RF2.3: cancela o download em curso; a versão instalada continua intacta.
#[tauri::command]
fn updater_cancel_download(app: AppHandle, u: Updates<'_>) -> updater::UpdateView {
    u.cancel_download(&app)
}

/// PRD 0018 RF2.2: "Lembrar depois" — o aviso some até a próxima verificação.
#[tauri::command]
fn updater_dismiss(app: AppHandle, u: Updates<'_>) -> updater::UpdateView {
    u.dismiss(&app)
}

/// PRD 0018 RF2.4: reinicia para terminar a atualização. Um login em andamento bloqueia
/// com `login_in_progress`; sem pacote pronto responde `no_update`. O backend para o
/// ciclo de atualização e persiste o estado antes de qualquer reinício.
#[tauri::command]
async fn updater_restart(app: AppHandle, b: Shared<'_>, u: Updates<'_>) -> Result<(), String> {
    let login = b.login_in_progress().await;
    u.restart(&app, login, || b.prepare_for_restart())
}

/// PRD 0020 RF1.2/RF1.3: open the Settings window (or focus it) on an optional
/// page and anchor of its sidebar registry. Async, as Tauri requires for a
/// command that creates a window (a synchronous one can deadlock on Windows).
#[tauri::command]
async fn open_settings(
    app: AppHandle,
    page: Option<String>,
    anchor: Option<String>,
) -> Result<(), String> {
    open_settings_window(&app, settings_window::Target::new(page, anchor));
    Ok(())
}

/// Every entry point lands here; the tray popover gives way to the window.
fn open_settings_window(app: &AppHandle, target: settings_window::Target) {
    if let Some(w) = app.get_webview_window("tray") {
        let _ = w.hide();
    }
    if let Err(e) = settings_window::open(app, target) {
        eprintln!("cuotai: settings window: {e}");
    }
}

/// The page and anchor the Settings window was opened on, read once by that
/// window (on load and after [`settings_window::NAVIGATE_EVENT`]).
#[tauri::command]
fn settings_target(app: AppHandle, window: WebviewWindow) -> Option<settings_window::Target> {
    (window.label() == settings_window::LABEL)
        .then(|| settings_window::take_target(&app))
        .flatten()
}

/// RF1.4: the Settings window titles itself in the interface language.
#[tauri::command]
fn set_settings_title(window: WebviewWindow, title: String) -> Result<(), String> {
    if window.label() != settings_window::LABEL {
        return Err("not_settings".into());
    }
    let title = settings_window::clean_title(&title);
    if title.is_empty() {
        return Ok(());
    }
    window.set_title(&title).map_err(|e| e.to_string())
}

/// PRD 0020 RF8.3: `Ctrl+W` closes the Settings window (macOS also has the menu item's `⌘W`).
#[tauri::command]
fn close_settings_window(window: WebviewWindow) -> Result<(), String> {
    if window.label() != settings_window::LABEL {
        return Err("not_settings".into());
    }
    window.close().map_err(|e| e.to_string())
}

/// macOS: the "Settings…" item (`⌘,`) of the app menu, kept so its text
/// follows the interface language.
#[cfg(target_os = "macos")]
struct AppMenuSettings(MenuItem<tauri::Wry>);

/// The id of that item; the tray menu's own item is `settings`.
#[cfg(target_os = "macos")]
const APP_MENU_SETTINGS: &str = "app-settings";

/// macOS: the default app menu (Edit, Window and the rest) with "Settings…"
/// after "About", as macOS apps place it.
#[cfg(target_os = "macos")]
fn app_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::default(app)?;
    let item = MenuItem::with_id(
        app,
        APP_MENU_SETTINGS,
        "Settings…",
        true,
        Some("CmdOrCtrl+,"),
    )?;
    if let Some(submenu) = menu
        .items()?
        .first()
        .and_then(|first| first.as_submenu().cloned())
    {
        submenu.insert(&item, 2)?;
        submenu.insert(&PredefinedMenuItem::separator(app)?, 3)?;
    }
    app.manage(AppMenuSettings(item));
    Ok(menu)
}

#[tauri::command]
fn show_main_window(app: AppHandle) {
    if let Some(w) = app.get_webview_window("tray") {
        let _ = w.hide();
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

fn toggle_popover(w: &WebviewWindow, anchor: PhysicalPosition<f64>) {
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        return;
    }
    let width = w.outer_size().map(|s| s.width as f64).unwrap_or(380.0);
    let height = w.outer_size().map(|s| s.height as f64).unwrap_or(600.0);
    let scale = w.scale_factor().unwrap_or(1.0);
    // The screen that holds the icon that was clicked, not the one where the
    // popover was last shown. The anchor is in physical pixels, so it is
    // matched against each monitor's physical rectangle: tao's
    // `monitor_from_point` compares with macOS points (`CGDisplayBounds`) and
    // would pick a screen to the right of a Retina one for an icon past the
    // middle of its menu bar.
    let rect = |m: &tauri::Monitor| {
        let (p, s) = (m.position(), m.size());
        (p.x as f64, p.y as f64, s.width as f64, s.height as f64)
    };
    let monitors: Vec<_> = w
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(rect)
        .collect();
    let screen = screen_containing(anchor, &monitors)
        .or_else(|| w.current_monitor().ok().flatten().map(|m| rect(&m)));
    let (x, y) = popover_origin(anchor, (width, height), screen, scale);
    let _ = w.set_position(PhysicalPosition::new(x, y));
    #[cfg(target_os = "macos")]
    macos_popover::present(w);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// The first screen, as `(x, y, width, height)` in physical pixels, whose
/// rectangle holds `anchor`.
fn screen_containing(
    anchor: PhysicalPosition<f64>,
    screens: &[(f64, f64, f64, f64)],
) -> Option<(f64, f64, f64, f64)> {
    screens.iter().copied().find(|&(x, y, w, h)| {
        anchor.x >= x && anchor.x < x + w && anchor.y >= y && anchor.y < y + h
    })
}

/// Where the popover's top-left corner goes, in physical pixels: centred on the
/// icon, below it when the icon is in the top half of its screen (macOS menu bar)
/// and above it otherwise (Windows/Linux taskbar), kept 8 px inside that screen.
/// `screen` is the screen's `(x, y, width, height)`; without it, only the gap
/// to the icon is applied.
fn popover_origin(
    anchor: PhysicalPosition<f64>,
    (width, height): (f64, f64),
    screen: Option<(f64, f64, f64, f64)>,
    scale: f64,
) -> (f64, f64) {
    let gap = 6.0 * scale;
    let below = screen
        .map(|(_, sy, _, sh)| anchor.y < sy + sh / 2.0)
        .unwrap_or(true);
    let x = anchor.x - width / 2.0;
    let y = if below {
        anchor.y + gap
    } else {
        anchor.y - height - gap
    };
    match screen {
        Some((sx, sy, sw, sh)) => {
            let margin = 8.0 * scale;
            let clamp = |v: f64, lo: f64, hi: f64| v.min(hi).max(lo);
            (
                clamp(x, sx + margin, sx + sw - width - margin),
                clamp(y, sy + margin, sy + sh - height - margin),
            )
        }
        None => (x, y),
    }
}

/// macOS: the tray popover must open over whatever Space is active, including
/// the Space of a full-screen app. A plain `NSWindow` cannot: it lives on the
/// Space where it was first shown, and showing it with focus activates Cuotai (a
/// Regular app with a Dock icon), which makes macOS switch to Cuotai's Space.
/// Collection behaviour and window level alone are not enough (tested).
///
/// So the popover becomes a non-activating panel, as menu-bar apps do: its class
/// is switched from tao's `TaoWindow` to an `NSPanel` subclass that can become
/// key (keyboard input still works), with the `NonactivatingPanel` style,
/// joining every Space as a full-screen auxiliary window at the pop-up menu
/// level. It is shown with `orderFrontRegardless` + `makeKeyWindow`, never
/// `NSApp.activate`, so the active app and Space stay put. Losing key status
/// still reaches tao's delegate (`windowDidResignKey:` gives `Focused(false)`),
/// which hides it. The main window, the Dock icon and the activation policy are
/// untouched.
#[cfg(target_os = "macos")]
mod macos_popover {
    use std::ffi::CStr;
    use std::sync::OnceLock;

    use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
    use objc2::{sel, ClassType};
    use objc2_app_kit::{
        NSPanel, NSPopUpMenuWindowLevel, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use tauri::WebviewWindow;

    const PANEL_CLASS: &CStr = c"CuotaiTrayPanel";

    /// The popover's Spaces behaviour: present on every Space (the active one at
    /// the moment of the click), allowed inside full-screen Spaces, kept still by
    /// Mission Control and left out of the window cycle.
    pub(super) fn collection_behavior() -> NSWindowCollectionBehavior {
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle
    }

    /// The popover's style: the window's own mask plus `NonactivatingPanel`.
    pub(super) fn style_mask(current: NSWindowStyleMask) -> NSWindowStyleMask {
        current | NSWindowStyleMask::NonactivatingPanel
    }

    extern "C-unwind" fn yes(_: &AnyObject, _: Sel) -> Bool {
        Bool::YES
    }

    extern "C-unwind" fn no(_: &AnyObject, _: Sel) -> Bool {
        Bool::NO
    }

    /// `NSPanel` subclass that can become key (a borderless panel cannot by
    /// default, which would break typing and the blur-to-hide rule) but never
    /// main. Registered once per process.
    fn panel_class() -> &'static AnyClass {
        static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
        CLASS.get_or_init(|| {
            if let Some(existing) = AnyClass::get(PANEL_CLASS) {
                return existing;
            }
            let mut builder = ClassBuilder::new(PANEL_CLASS, NSPanel::class())
                .expect("CuotaiTrayPanel is registered only here");
            // SAFETY: both selectors return BOOL and take no argument, matching
            // the `extern "C-unwind" fn(&AnyObject, Sel) -> Bool` signatures.
            unsafe {
                builder.add_method(
                    sel!(canBecomeKeyWindow),
                    yes as extern "C-unwind" fn(_, _) -> _,
                );
                builder.add_method(
                    sel!(canBecomeMainWindow),
                    no as extern "C-unwind" fn(_, _) -> _,
                );
            }
            // tao's `TaoWindow` (an `NSWindow` subclass) adds one `BOOL` ivar,
            // `focusable`; `NSPanel` adds none. Declaring the same ivar keeps the
            // instance size and the ivar's offset identical, so the class swap is
            // layout-compatible and tao's `set_focusable` keeps working.
            builder.add_ivar::<Bool>(c"focusable");
            builder.register()
        })
    }

    /// Whether an object of class `from` can become a `to`: same instance size
    /// and, when `from` declares tao's `focusable` ivar, `to` holds it at the
    /// same offset.
    pub(super) fn same_layout(from: &AnyClass, to: &AnyClass) -> bool {
        if from.instance_size() != to.instance_size() {
            return false;
        }
        match (
            from.instance_variable(c"focusable"),
            to.instance_variable(c"focusable"),
        ) {
            (Some(a), Some(b)) => a.offset() == b.offset(),
            (None, _) => true,
            (Some(_), None) => false,
        }
    }

    #[cfg(test)]
    pub(super) fn panel_class_for_tests() -> &'static AnyClass {
        panel_class()
    }

    fn ns_window(w: &WebviewWindow) -> Option<&NSWindow> {
        let ptr = w.ns_window().ok()?;
        // SAFETY: `ns_window()` returns the window's live `NSWindow`, kept alive
        // by the `WebviewWindow` borrowed for the returned lifetime; callers run on
        // the main thread (`setup` and tray-icon events), as AppKit requires.
        (!ptr.is_null()).then(|| unsafe { &*ptr.cast::<NSWindow>() })
    }

    /// Turns the popover into a non-activating panel. Called once from `setup`,
    /// on the main thread, before it is ever shown.
    pub(super) fn make_panel(w: &WebviewWindow) {
        let Some(window) = ns_window(w) else { return };
        let object: &AnyObject = window.as_ref();
        let (from, to) = (object.class(), panel_class());
        if !same_layout(from, to) {
            // A tao release changed `TaoWindow`'s ivars: never swap an
            // incompatible layout. The popover keeps working as a plain window
            // (without the full-screen overlay) instead of corrupting memory.
            window.setCollectionBehavior(collection_behavior());
            window.setLevel(NSPopUpMenuWindowLevel);
            return;
        }
        // SAFETY: `same_layout` checked that both classes have the same instance
        // size and the `focusable` ivar at the same offset, and `CuotaiTrayPanel`
        // descends from `NSWindow` like `TaoWindow` (through `NSPanel`, which adds
        // no ivars), so the object stays valid (the swap tauri-nspanel performs).
        // `TaoWindow`'s `canBecomeKeyWindow`/`canBecomeMainWindow` are replaced
        // above and its `sendEvent:` override only serves movable-by-background
        // windows, which the popover is not. The window delegate, and with it
        // every tao window event, is unchanged.
        unsafe { AnyObject::set_class(object, to) };
        // SAFETY: the object is now an `NSPanel`.
        let panel: &NSPanel = unsafe { &*(window as *const NSWindow).cast::<NSPanel>() };
        panel.setStyleMask(style_mask(panel.styleMask()));
        // `setFloatingPanel` sets the floating level, so the level comes after it.
        panel.setFloatingPanel(true);
        panel.setBecomesKeyOnlyIfNeeded(false);
        panel.setHidesOnDeactivate(false);
        panel.setCollectionBehavior(collection_behavior());
        panel.setLevel(NSPopUpMenuWindowLevel);
    }

    /// Shows the popover on the active Space and gives it the keyboard without
    /// activating Cuotai, so a full-screen app keeps its Space.
    pub(super) fn present(w: &WebviewWindow) {
        match ns_window(w) {
            Some(window) => {
                window.orderFrontRegardless();
                window.makeKeyWindow();
            }
            None => {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
    }
}

/// The icon tooltip, plus the next reset notice on a second line.
fn tray_tooltip(p: &TrayMenuPayload) -> String {
    let tooltip = clean_menu_line(&p.tooltip);
    match p
        .notice
        .as_deref()
        .map(clean_menu_line)
        .filter(|n| !n.is_empty())
    {
        Some(notice) => format!("{tooltip}\n{notice}"),
        None => tooltip,
    }
}

/// The tray icon before the first state arrives (PRD 0025 RF3 redraws it).
fn tray_icon() -> tauri::Result<tauri::image::Image<'static>> {
    tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))
}

pub fn run() {
    let backend = Arc::new(Backend::new(
        notify::Notices::load(),
        settings::status_queries(),
        settings::serve_prefs(),
        settings::hooks_enabled(),
        settings::widget_prefs(),
    ));
    // PRD 0015 RF4.3: the local server publishes this computer's local cost.
    let local_cost = Arc::new(local_cost::LocalCost::new());
    backend.publish_local_cost(Arc::clone(&local_cost));
    // PRD 0018 RF2.6/RF2.5: the update service reads `desktop.json`, detects the
    // install source and keeps both scheduled and manual requests behind the persisted
    // one-time answer before the first possible request.
    let updates = Arc::new(updater::Service::new());
    let builder = tauri::Builder::default();
    // PRD 0020 RF1.3: "Settings…" with ⌘, in the macOS app menu; Windows and
    // Linux have no app menu, and the interface handles Ctrl+, itself.
    #[cfg(target_os = "macos")]
    let builder = builder.menu(app_menu).on_menu_event(|app, event| {
        if event.id() == APP_MENU_SETTINGS {
            open_settings_window(app, settings_window::Target::default());
        }
    });
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        // PRD 0018 RF1.1: official updater plugin (2.x). The web UI never calls it — the
        // shell exposes the commands above — so no `updater:` capability is needed.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Arc::clone(&backend))
        .manage(local_cost)
        .manage(Arc::clone(&updates))
        .manage(settings_window::State::new(settings::settings_path()))
        .invoke_handler(tauri::generate_handler![
            get_view,
            tray_preview,
            tray_accounts,
            set_tray_prefs,
            set_advanced_open,
            first_run_turn_on,
            first_run_record,
            refresh_all,
            refresh_account,
            list_providers,
            search_providers,
            provider_modes,
            detect_sources,
            set_detect_enabled,
            dismiss_detected,
            list_extensions,
            extension_disclosure,
            enable_extension,
            disable_extension,
            test_extension,
            form_schema,
            visible_fields,
            field_choices,
            draft_access,
            get_account,
            migrate_session,
            forget_session,
            vault_audit,
            vault_prune,
            preview_session,
            test_session,
            test_aggregator,
            open_aggregator_dashboard,
            save_account,
            delete_account,
            retry_purge,
            set_refresh_seconds,
            move_account,
            reload_config,
            set_config_file,
            updater_status,
            updater_set_automatic,
            updater_answer_automatic,
            updater_check_now,
            updater_install,
            updater_cancel_download,
            updater_dismiss,
            updater_restart,
            about_info,
            whats_new,
            open_external,
            report_problem,
            config_changed_on_disk,
            login_start,
            login_submit_code,
            login_cancel,
            model_state,
            load_models,
            sync_models,
            model_support,
            load_user_content,
            core_catalog,
            get_settings,
            set_tray_menu,
            set_user_dir,
            open_user_dir,
            set_motion,
            set_show_pace,
            set_preference,
            migrate_local_storage,
            set_history_days,
            history_export,
            history_erase,
            set_history_recording,
            set_status_enabled,
            serve_status,
            set_serve,
            generate_serve_token,
            revoke_serve_token,
            copy_serve_token,
            hooks_status,
            set_hooks_enabled,
            confirm_hooks,
            save_hooks,
            test_hook,
            widgets_status,
            set_widgets,
            delete_widget_data,
            cost_status,
            cost_usage,
            cost_enable,
            cost_disable,
            cost_set_paths,
            cost_set_options,
            cost_prices,
            cost_erase,
            history_size,
            account_history,
            stats,
            reset_timeline,
            set_notify_enabled,
            set_alerts_enabled,
            set_alert_options,
            set_metric_alert,
            set_pool_alerts,
            set_notify_options,
            set_account_notify,
            set_locale,
            set_callback_style,
            send_test_notification,
            open_settings,
            settings_target,
            set_settings_title,
            close_settings_window,
            show_main_window,
            quit_app
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);

            let icon = tray_icon()?;
            TrayIconBuilder::with_id("cuotai")
                .icon(icon)
                // PRD 0025 RF3.1: a template on macOS (light and dark menu bars).
                .icon_as_template(cfg!(target_os = "macos"))
                .tooltip("Cuotai")
                // Clique esquerdo abre o popover; o direito mostra o menu de texto (contas e ações).
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "account" | "open" => show_main_window(app.clone()),
                    // PRD 0018 RF2.2: the update line opens the window, where the notice,
                    // the release notes and the buttons live.
                    "update" => show_main_window(app.clone()),
                    "settings" => open_settings_window(app, settings_window::Target::default()),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        position,
                        ..
                    } = event
                    {
                        if let Some(w) = tray.app_handle().get_webview_window("tray") {
                            toggle_popover(&w, position);
                        }
                    }
                })
                .build(app)?;
            // Menu inicial (a interface o substitui, já traduzido e com as contas, assim que carrega).
            let initial = TrayMenuPayload {
                lines: vec![],
                empty: "Cuotai".into(),
                open: "Abrir".into(),
                settings: "Configurações".into(),
                quit: "Sair".into(),
                tooltip: "Cuotai".into(),
                notice: None,
                update: None,
            };
            if let (Some(tray), Ok(menu)) = (
                app.tray_by_id("cuotai"),
                build_tray_menu(app.handle(), &initial),
            ) {
                let _ = tray.set_menu(Some(menu));
            }

            // Popover fecha ao perder o foco; fechar a janela principal só a esconde.
            if let Some(w) = app.get_webview_window("tray") {
                #[cfg(target_os = "macos")]
                macos_popover::make_panel(&w);
                let handle = w.clone();
                w.on_window_event(move |e| {
                    if let WindowEvent::Focused(false) = e {
                        let _ = handle.hide();
                    }
                });
            }
            if let Some(w) = app.get_webview_window("main") {
                let handle = w.clone();
                w.on_window_event(move |e| {
                    if let WindowEvent::CloseRequested { api, .. } = e {
                        api.prevent_close();
                        let _ = handle.hide();
                    }
                });
            }
            // PRD 0015 RF2.2: the local cost scan, on a thread of its own.
            app.state::<Arc<local_cost::LocalCost>>().start();
            backend.notify_hook_runs(app.handle().clone());
            tauri::async_runtime::spawn(Arc::clone(&backend).run_scheduler(app.handle().clone()));
            tauri::async_runtime::spawn(Arc::clone(&backend).run_notices(app.handle().clone()));
            tauri::async_runtime::spawn(Arc::clone(&backend).run_incidents(app.handle().clone()));
            tauri::async_runtime::spawn(Arc::clone(&backend).start_serve(app.handle().clone()));
            // PRD 0014 RF1.4: the widget snapshot publisher, on a task of
            // its own so a refresh never waits for a file write.
            tauri::async_runtime::spawn(Arc::clone(&backend).run_widgets());
            // PRD 0018 RF2.1/RF2.6: the update check on opening and every 24 h starts
            // only after the affirmative one-time answer and while no preference or
            // package policy turns it off.
            tauri::async_runtime::spawn(Arc::clone(&updates).run(app.handle().clone()));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Cuotai");
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_popover_joins_full_screen_spaces() {
        use objc2_app_kit::NSWindowCollectionBehavior as B;
        let b = super::macos_popover::collection_behavior();
        for flag in [
            B::CanJoinAllSpaces,
            B::FullScreenAuxiliary,
            B::Stationary,
            B::IgnoresCycle,
        ] {
            assert!(b.contains(flag), "missing {flag:?}");
        }
        // Mutually exclusive with CanJoinAllSpaces, and the popover is never a
        // full-screen window of its own.
        for flag in [B::MoveToActiveSpace, B::FullScreenPrimary] {
            assert!(!b.contains(flag), "unexpected {flag:?}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_popover_is_non_activating_and_keeps_its_style() {
        use objc2_app_kit::NSWindowStyleMask as M;
        let m = super::macos_popover::style_mask(M::Borderless | M::FullSizeContentView);
        assert!(m.contains(M::NonactivatingPanel));
        assert!(m.contains(M::FullSizeContentView));
        assert!(!m.contains(M::Titled));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_panel_class_matches_tao_window_layout() {
        use objc2::runtime::{AnyClass, Bool, ClassBuilder};
        use objc2::ClassType;
        use objc2_app_kit::{NSPanel, NSWindow};
        // The same shape as tao's `TaoWindow`: `NSWindow` plus a BOOL `focusable`.
        let tao_like = AnyClass::get(c"CuotaiTestTaoWindow").unwrap_or_else(|| {
            let mut b = ClassBuilder::new(c"CuotaiTestTaoWindow", NSWindow::class()).unwrap();
            b.add_ivar::<Bool>(c"focusable");
            b.register()
        });
        let panel = super::macos_popover::panel_class_for_tests();
        assert_eq!(panel.superclass(), Some(NSPanel::class()));
        assert!(super::macos_popover::same_layout(tao_like, panel));
        // A window class with more state than the panel: the swap is refused.
        // (A plain NSWindow is not used here: whether one BOOL changes the
        // instance size depends on the macOS version's padding.)
        let bigger = AnyClass::get(c"CuotaiTestBiggerWindow").unwrap_or_else(|| {
            let mut b = ClassBuilder::new(c"CuotaiTestBiggerWindow", NSWindow::class()).unwrap();
            b.add_ivar::<Bool>(c"focusable");
            b.add_ivar::<[u64; 4]>(c"extra");
            b.register()
        });
        assert!(!super::macos_popover::same_layout(bigger, panel));
    }

    #[test]
    fn popover_opens_below_a_menu_bar_icon_on_its_own_screen() {
        use super::popover_origin;
        use tauri::PhysicalPosition;
        // Second screen to the right of the main one, icon in its menu bar.
        let screen = Some((2880.0, 0.0, 3024.0, 1964.0));
        let (x, y) = popover_origin(
            PhysicalPosition::new(4000.0, 20.0),
            (760.0, 1200.0),
            screen,
            2.0,
        );
        assert_eq!((x, y), (3620.0, 32.0));
        // Near the right edge: kept 8 pt inside that screen.
        let (x, _) = popover_origin(
            PhysicalPosition::new(5890.0, 20.0),
            (760.0, 1200.0),
            screen,
            2.0,
        );
        assert_eq!(x, 2880.0 + 3024.0 - 760.0 - 16.0);
    }

    #[test]
    fn popover_screen_is_found_in_physical_pixels() {
        use super::screen_containing;
        use tauri::PhysicalPosition;
        // A 1512x982 pt Retina screen with a 2560x1440 pt Retina screen to its
        // right, as tao reports them (points times each screen's scale).
        let main = (0.0, 0.0, 3024.0, 1964.0);
        let right = (3024.0, 0.0, 5120.0, 2880.0);
        let screens = [main, right];
        // An icon at x = 1300 pt of the main menu bar arrives as x = 2600 px:
        // in points it would fall on the right screen; in pixels it stays home.
        assert_eq!(
            screen_containing(PhysicalPosition::new(2600.0, 40.0), &screens),
            Some(main)
        );
        assert_eq!(
            screen_containing(PhysicalPosition::new(4000.0, 40.0), &screens),
            Some(right)
        );
        assert_eq!(
            screen_containing(PhysicalPosition::new(9000.0, 40.0), &screens),
            None
        );
    }

    #[test]
    fn popover_opens_above_a_taskbar_icon() {
        use super::popover_origin;
        use tauri::PhysicalPosition;
        let screen = Some((0.0, 0.0, 1920.0, 1080.0));
        let (x, y) = popover_origin(
            PhysicalPosition::new(1800.0, 1060.0),
            (380.0, 600.0),
            screen,
            1.0,
        );
        assert_eq!((x, y), (1920.0 - 380.0 - 8.0, 1060.0 - 600.0 - 6.0));
        // Without a known screen only the gap is applied.
        let (x, y) = popover_origin(
            PhysicalPosition::new(100.0, 10.0),
            (380.0, 600.0),
            None,
            1.0,
        );
        assert_eq!((x, y), (-90.0, 16.0));
    }

    use super::clean_menu_line;

    #[test]
    fn menu_lines_are_trimmed_stripped_of_controls_and_capped() {
        assert_eq!(clean_menu_line("  Claude Code: 56%  "), "Claude Code: 56%");
        assert_eq!(clean_menu_line("Kiro\n\tAI: 94%"), "KiroAI: 94%");
        let long = "x".repeat(200);
        let cut = clean_menu_line(&long);
        assert_eq!(cut.chars().count(), 80);
        assert!(cut.ends_with('…'));
    }
}
