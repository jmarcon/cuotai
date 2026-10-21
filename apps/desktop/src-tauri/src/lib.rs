//! Shell Tauri do Cuotai: janela principal, popover na bandeja e comandos.
//! Somente consome `cuotai-core` (via `backend.rs`); não contém lógica de provider.
mod about;
mod backend;
mod hooks;
mod local_cost;
mod local_server;
mod notify;
mod settings;
mod user_content;

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
    let menu = build_tray_menu(&app, &payload).map_err(|e| e.to_string())?;
    if let Some(tray) = app.tray_by_id("cuotai") {
        tray.set_menu(Some(menu)).map_err(|e| e.to_string())?;
        tray.set_tooltip(Some(tray_tooltip(&payload)))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn about_info() -> about::AboutView {
    about::view()
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
/// user chose in the native save dialog. Returns the rows written.
#[tauri::command]
async fn history_export(
    b: Shared<'_>,
    index: Option<usize>,
    days: Option<u32>,
    format: cuotai_core::history::ExportFormat,
    path: String,
) -> Result<usize, String> {
    b.history_export(index, days, format, std::path::PathBuf::from(path))
        .await
}

/// PRD 0006 RF4.4: erase the history of account `index`, or all of it with
/// `None`, after the confirmation the UI asked for. Returns the samples removed.
#[tauri::command]
async fn history_erase(
    app: AppHandle,
    b: Shared<'_>,
    index: Option<usize>,
) -> Result<usize, String> {
    b.history_erase(&app, index).await
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
) -> Result<(), String> {
    b.update_notices(&app, |n| {
        let alerts = n.alerts().with_choices(&thresholds, exhaustion, recovery)?;
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
    b.update_notices(&app, |n| n.set_locale(&locale)).await
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

#[tauri::command]
fn open_settings(app: AppHandle) {
    show_main_window(app.clone());
    let _ = app.emit("open-settings", ());
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
    // Ícone no topo da tela (macOS): abre abaixo; na base (Windows/Linux): abre acima.
    let below = w
        .current_monitor()
        .ok()
        .flatten()
        .map(|m| anchor.y < m.size().height as f64 / 2.0)
        .unwrap_or(true);
    let y = if below {
        anchor.y + 6.0 * scale
    } else {
        anchor.y - height - 6.0 * scale
    };
    let _ = w.set_position(PhysicalPosition::new(
        (anchor.x - width / 2.0).max(8.0),
        y.max(8.0),
    ));
    let _ = w.show();
    let _ = w.set_focus();
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

const TRAY_PNG: &[u8] = include_bytes!("../icons/tray.png");

/// PRD 0011 RF4.2: paint the outage indicator on the tray icon's RGBA
/// pixels: a red dot with a light ring in the bottom-right corner, a third
/// of the icon wide, so it reads on light and dark menu bars.
fn paint_outage_dot(rgba: &mut [u8], width: u32, height: u32) {
    let size = width.min(height) as f32;
    let radius = size / 6.0;
    let ring = (size / 22.0).max(1.0);
    let (cx, cy) = (width as f32 - radius - ring, height as f32 - radius - ring);
    for y in 0..height {
        for x in 0..width {
            let distance = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy);
            let color = if distance <= radius {
                [0xe5, 0x48, 0x4d, 0xff]
            } else if distance <= radius + ring {
                [0xff, 0xff, 0xff, 0xff]
            } else {
                continue;
            };
            let at = ((y * width + x) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&color);
        }
    }
}

/// The tray icon, with the outage indicator when `outage`.
fn tray_icon(outage: bool) -> tauri::Result<tauri::image::Image<'static>> {
    let icon = tauri::image::Image::from_bytes(TRAY_PNG)?;
    if !outage {
        return Ok(icon);
    }
    let (width, height) = (icon.width(), icon.height());
    let mut rgba = icon.rgba().to_vec();
    paint_outage_dot(&mut rgba, width, height);
    Ok(tauri::image::Image::new_owned(rgba, width, height))
}

/// PRD 0011 RF4.2: show or clear the outage indicator on the tray icon.
pub(crate) fn show_tray_outage(app: &AppHandle, outage: bool) {
    if let (Some(tray), Ok(icon)) = (app.tray_by_id("cuotai"), tray_icon(outage)) {
        let _ = tray.set_icon(Some(icon));
    }
}

pub fn run() {
    let backend = Arc::new(Backend::new(
        notify::Notices::load(),
        settings::status_queries(),
        settings::serve_prefs(),
        settings::hooks_enabled(),
    ));
    // PRD 0015 RF4.3: the local server publishes this computer's local cost.
    let local_cost = Arc::new(local_cost::LocalCost::new());
    backend.publish_local_cost(Arc::clone(&local_cost));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(Arc::clone(&backend))
        .manage(local_cost)
        .invoke_handler(tauri::generate_handler![
            get_view,
            refresh_all,
            refresh_account,
            list_providers,
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
            preview_session,
            test_session,
            save_account,
            delete_account,
            retry_purge,
            set_refresh_seconds,
            move_account,
            reload_config,
            set_config_file,
            about_info,
            open_external,
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
            set_notify_enabled,
            set_alerts_enabled,
            set_alert_options,
            set_metric_alert,
            set_notify_options,
            set_account_notify,
            set_locale,
            send_test_notification,
            open_settings,
            show_main_window,
            quit_app
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);

            let icon = tray_icon(false)?;
            TrayIconBuilder::with_id("cuotai")
                .icon(icon)
                .icon_as_template(false)
                .tooltip("Cuotai")
                // Clique esquerdo abre o popover; o direito mostra o menu de texto (contas e ações).
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "account" | "open" => show_main_window(app.clone()),
                    "settings" => open_settings(app.clone()),
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
            };
            if let (Some(tray), Ok(menu)) = (
                app.tray_by_id("cuotai"),
                build_tray_menu(app.handle(), &initial),
            ) {
                let _ = tray.set_menu(Some(menu));
            }

            // Popover fecha ao perder o foco; fechar a janela principal só a esconde.
            if let Some(w) = app.get_webview_window("tray") {
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
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Cuotai");
}

#[cfg(test)]
mod tests {
    use super::{clean_menu_line, paint_outage_dot};

    #[test]
    fn outage_dot_marks_only_the_bottom_right_corner() {
        let (width, height) = (44, 44);
        let blank = vec![0u8; (width * height * 4) as usize];
        let mut rgba = blank.clone();
        paint_outage_dot(&mut rgba, width, height);
        let pixel = |x: u32, y: u32| {
            let at = ((y * width + x) * 4) as usize;
            <[u8; 4]>::try_from(&rgba[at..at + 4]).unwrap()
        };
        // Red in the dot, a light ring around it, the logo untouched.
        assert_eq!(pixel(35, 35), [0xe5, 0x48, 0x4d, 0xff]);
        assert_eq!(pixel(43, 35)[..3], [0xff, 0xff, 0xff]);
        for (x, y) in [(0, 0), (22, 22), (43, 0), (0, 43), (22, 30)] {
            assert_eq!(pixel(x, y), [0, 0, 0, 0], "({x}, {y})");
        }
        assert_ne!(rgba, blank);
    }

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
