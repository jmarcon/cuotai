//! Shell Tauri do Cuotai: janela principal, popover na bandeja e ponte de dados.
//! Somente consome `cuotai-core`; não contém lógica de provider.
use cuotai_core::{
    oauth::OAuthSession, Config, EnvironmentCredentials, Monitor, Report, Snapshot,
};
use std::{path::PathBuf, sync::Arc};
use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, PhysicalPosition, WebviewWindow, WindowEvent,
};
use tauri::async_runtime::Mutex;

#[derive(Default)]
struct State {
    monitor: Mutex<Option<(Arc<Monitor>, usize)>>,
}

fn config_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CUOTAI_CONFIG") {
        return Some(p.into());
    }
    let local = PathBuf::from("cuotai.toml");
    if local.exists() {
        return Some(local);
    }
    let os = dirs::config_dir()?.join("cuotai").join("config.toml");
    os.exists().then_some(os)
}

async fn monitor(state: &State) -> Result<(Arc<Monitor>, usize), String> {
    let mut guard = state.monitor.lock().await;
    if let Some((m, n)) = guard.as_ref() {
        return Ok((Arc::clone(m), *n));
    }
    let path = config_path().ok_or("Nenhum cuotai.toml encontrado (use CUOTAI_CONFIG).")?;
    let text = std::fs::read_to_string(&path).map_err(|_| "Não foi possível ler a configuração.")?;
    let config = Config::from_toml(&text).map_err(|e| e.to_string())?;
    let count = config.accounts.len();
    let session = Arc::new(OAuthSession::system().map_err(|_| "Cofre de credenciais indisponível.")?);
    let m = Arc::new(
        Monitor::with_oauth(config, Arc::new(EnvironmentCredentials), session)
            .map_err(|e| e.to_string())?,
    );
    *guard = Some((Arc::clone(&m), count));
    Ok((m, count))
}

#[tauri::command]
async fn get_snapshot(state: tauri::State<'_, State>) -> Result<Snapshot, String> {
    Ok(monitor(&state).await?.0.refresh().await)
}

#[tauri::command]
async fn refresh_account(state: tauri::State<'_, State>, index: usize) -> Result<Report, String> {
    let (m, count) = monitor(&state).await?;
    if index >= count {
        return Err("Conta inexistente.".into());
    }
    Ok(m.refresh_account(index).await)
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
    let scale = w.scale_factor().unwrap_or(1.0);
    // Centraliza sob o ícone; se o ícone está na base da tela (Windows/Linux), sobe o popover.
    let below = w
        .current_monitor()
        .ok()
        .flatten()
        .map(|m| anchor.y < m.size().height as f64 / 2.0)
        .unwrap_or(true);
    let height = w.outer_size().map(|s| s.height as f64).unwrap_or(600.0);
    let y = if below { anchor.y + 6.0 * scale } else { anchor.y - height - 6.0 * scale };
    let _ = w.set_position(PhysicalPosition::new((anchor.x - width / 2.0).max(8.0), y.max(8.0)));
    let _ = w.show();
    let _ = w.set_focus();
}

pub fn run() {
    tauri::Builder::default()
        .manage(State::default())
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            refresh_account,
            show_main_window,
            quit_app
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);

            let icon = app.default_window_icon().cloned().ok_or("ícone ausente")?;
            TrayIconBuilder::with_id("cuotai")
                .icon(icon)
                .icon_as_template(false)
                .tooltip("Cuotai")
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
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Cuotai");
}
