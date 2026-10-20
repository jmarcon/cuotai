//! Shell Tauri do Cuotai: janela principal, popover na bandeja e comandos.
//! Somente consome `cuotai-core` (via `backend.rs`); não contém lógica de provider.
mod about;
mod backend;
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
) -> backend::AccessView {
    backend::draft_access(provider, &auth_mode, &fields)
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
        tray.set_tooltip(Some(clean_menu_line(&payload.tooltip)))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn about_info() -> about::AboutView {
    about::view()
}

/// Abre no navegador um repositório da lista do "Sobre"; qualquer outro endereço é recusado.
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

pub fn run() {
    let backend = Arc::new(Backend::default());
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::clone(&backend))
        .invoke_handler(tauri::generate_handler![
            get_view,
            refresh_all,
            refresh_account,
            list_providers,
            form_schema,
            visible_fields,
            field_choices,
            draft_access,
            get_account,
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
            history_size,
            account_history,
            open_settings,
            show_main_window,
            quit_app
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);

            let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
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
            tauri::async_runtime::spawn(Arc::clone(&backend).run_scheduler(app.handle().clone()));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Cuotai");
}

#[cfg(test)]
mod tests {
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
