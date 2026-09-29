use crate::domain::AppInfo;

#[tauri::command]
pub fn get_app_info() -> AppInfo {
    AppInfo {
        name: "JIRA Agent Desktop".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        tauri_version: tauri::VERSION.to_string(),
        platform: std::env::consts::OS.to_string(),
    }
}
