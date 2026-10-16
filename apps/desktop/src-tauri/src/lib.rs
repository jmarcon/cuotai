//! Shell Tauri do Cuotai: janela principal, popover na bandeja e comandos.
//! Somente consome `cuotai-core` (via `backend.rs`); não contém lógica de provider.
mod backend;
mod settings;
mod user_content;

use backend::{
    AccountForm, AccountInput, Backend, FormSchema, LoginStartInput, ProviderInfo, View,
};
use cuotai_core::Provider;
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
async fn save_account(app: AppHandle, b: Shared<'_>, input: AccountInput) -> Result<(), String> {
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

#[tauri::command]
async fn set_refresh_seconds(app: AppHandle, b: Shared<'_>, seconds: u64) -> Result<(), String> {
    b.set_refresh_seconds(&app, seconds).await
}

#[tauri::command]
async fn move_account(
    app: AppHandle,
    b: Shared<'_>,
    from: usize,
    to: usize,
) -> Result<(), String> {
    b.move_account(&app, from, to).await
}

#[tauri::command]
fn load_user_content() -> Result<user_content::UserContent, String> {
    user_content::load().ok_or_else(|| "Pasta de configuração indisponível.".to_string())
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
        menu.append(&MenuItem::with_id(app, "none", clean_menu_line(&p.empty), false, None::<&str>)?)?;
    }
    // Habilitadas (texto com contraste normal); clicar numa conta abre a janela principal.
    for line in &lines {
        menu.append(&MenuItem::with_id(app, "account", line, true, None::<&str>)?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "open", clean_menu_line(&p.open), true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "settings", clean_menu_line(&p.settings), true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "quit", clean_menu_line(&p.quit), true, None::<&str>)?)?;
    Ok(menu)
}

/// Atualiza o menu do clique direito. Comando síncrono: roda na thread principal, onde o menu nativo vive.
#[tauri::command]
fn set_tray_menu(app: AppHandle, payload: TrayMenuPayload) -> Result<(), String> {
    let menu = build_tray_menu(&app, &payload).map_err(|e| e.to_string())?;
    if let Some(tray) = app.tray_by_id("cuotai") {
        tray.set_menu(Some(menu)).map_err(|e| e.to_string())?;
    }
    Ok(())
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
            get_account,
            save_account,
            delete_account,
            set_refresh_seconds,
            move_account,
            login_start,
            login_submit_code,
            login_cancel,
            load_user_content,
            get_settings,
            set_tray_menu,
            set_user_dir,
            open_user_dir,
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
            };
            if let (Some(tray), Ok(menu)) = (app.tray_by_id("cuotai"), build_tray_menu(app.handle(), &initial)) {
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
