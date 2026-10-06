//! System tray icon: a compact status window (pending tickets / running agents)
//! with quick actions. The menu labels are updated from the frontend via
//! [`crate::commands::set_tray_stats`]; everything else is handled in Rust so no
//! extra window/tray permissions are exposed to the webview.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, Wry};

const TRAY_ID: &str = "main";
/// Emitted when the tray asks the frontend to navigate (currently Settings).
pub const NAVIGATE_EVENT: &str = "tray://navigate";

/// The two menu items whose text tracks application state. Kept in managed
/// state so [`set_stats`] can update them without re-walking the menu.
pub struct TrayStats {
    pending: MenuItem<Wry>,
    running: MenuItem<Wry>,
}

pub fn pending_text(pending: u32) -> String {
    if pending == 1 {
        "1 pending ticket".to_string()
    } else {
        format!("{pending} pending tickets")
    }
}

pub fn running_text(running: u32) -> String {
    if running == 1 {
        "1 agent running".to_string()
    } else {
        format!("{running} agents running")
    }
}

/// Builds the tray icon and menu. Must run after `AppState` is managed, since
/// the menu actions need the state handle.
pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let title = MenuItem::with_id(app, "title", "JIRA Agent", false, None::<&str>)?;
    let pending = MenuItem::with_id(app, "stats", pending_text(0), false, None::<&str>)?;
    let running = MenuItem::with_id(app, "runs", running_text(0), false, None::<&str>)?;
    let separator_one = PredefinedMenuItem::separator(app)?;
    let show = MenuItem::with_id(app, "show", "Show window", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let separator_two = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &title,
            &pending,
            &running,
            &separator_one,
            &show,
            &settings,
            &separator_two,
            &quit,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("JIRA Agent")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "settings" => {
                show_main(app);
                let _ = app.emit(NAVIGATE_EVENT, "/settings");
            }
            // Routes through the normal exit path so agent servers are stopped
            // and their tracked rows removed.
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder.build(app)?;
    app.manage(TrayStats { pending, running });
    Ok(())
}

/// Shows, restores and focuses the main window.
pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Updates the two dynamic menu labels.
pub fn set_stats<R: Runtime>(app: &AppHandle<R>, pending: u32, running: u32) {
    if let Some(stats) = app.try_state::<TrayStats>() {
        let _ = stats.pending.set_text(pending_text(pending));
        let _ = stats.running.set_text(running_text(running));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_stats_labels_with_correct_pluralisation() {
        assert_eq!(pending_text(0), "0 pending tickets");
        assert_eq!(pending_text(1), "1 pending ticket");
        assert_eq!(pending_text(3), "3 pending tickets");
        assert_eq!(running_text(0), "0 agents running");
        assert_eq!(running_text(1), "1 agent running");
        assert_eq!(running_text(2), "2 agents running");
    }
}
