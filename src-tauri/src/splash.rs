use std::time::Duration;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent, window::Color};

const SPLASH: &str = "splash";
const MAIN: &str = "main";
/// Shows the main window anyway if the frontend never reports in, so a startup error stays visible.
const SHOW_MAIN_AFTER: Duration = Duration::from_secs(15);

pub fn open(app: &AppHandle) -> tauri::Result<()> {
    let version = app.package_info().version.to_string();
    let splash = WebviewWindowBuilder::new(app, SPLASH, WebviewUrl::App("splash.html".into()))
        .title("Fran DB")
        .inner_size(300.0, 340.0)
        .resizable(false)
        .maximizable(false)
        .decorations(false)
        .center()
        .background_color(Color(14, 15, 18, 255))
        .initialization_script(format!(
            "document.addEventListener('DOMContentLoaded', () => {{ document.getElementById('version').textContent = 'v{version}'; }});"
        ))
        .build()?;

    // Closing the splash yourself quits, instead of leaving the hidden main window running.
    let handle = app.clone();
    splash.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { .. } = event {
            handle.exit(0);
        }
    });

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(SHOW_MAIN_AFTER).await;
        if handle.get_webview_window(SPLASH).is_some() {
            reveal_main(&handle);
        }
    });

    Ok(())
}

fn reveal_main(app: &AppHandle) {
    if let Some(main) = app.get_webview_window(MAIN) {
        let _ = main.show();
        let _ = main.set_focus();
    }
    if let Some(splash) = app.get_webview_window(SPLASH) {
        let _ = splash.destroy();
    }
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) {
    reveal_main(&app);
}
