mod device;

use std::path::PathBuf;

use device::{AppError, AppState, ConnectEndpoint, DeviceManager, Point, TapReceipt};
use tauri::{Manager, State, ipc::Response};

#[tauri::command]
async fn get_app_app_state(manager: State<'_, DeviceManager>) -> Result<AppState, AppError> {
    manager.initialize().await
}

#[tauri::command]
async fn set_adb_path(
    path: String,
    manager: State<'_, DeviceManager>,
) -> Result<AppState, AppError> {
    manager.set_adb_path(PathBuf::from(path)).await
}

#[tauri::command]
async fn refresh_devices(manager: State<'_, DeviceManager>) -> Result<AppState, AppError> {
    manager.refresh_devices().await
}

#[tauri::command]
async fn connect_device(
    endpoint: ConnectEndpoint,
    manager: State<'_, DeviceManager>,
) -> Result<AppState, AppError> {
    manager.connect_device(endpoint).await
}

#[tauri::command]
async fn select_device(
    serial: String,
    manager: State<'_, DeviceManager>,
) -> Result<AppState, AppError> {
    manager.select_device(serial).await
}

#[tauri::command]
async fn capture_screen(manager: State<'_, DeviceManager>) -> Result<Response, AppError> {
    manager.capture_screen().await.map(Response::new)
}

#[tauri::command]
async fn tap_screen(
    point: Point,
    manager: State<'_, DeviceManager>,
) -> Result<TapReceipt, AppError> {
    manager.tap_screen(point).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let config_path = app.path().app_config_dir()?.join("config.json");
            app.manage(DeviceManager::new(config_path));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_app_state,
            set_adb_path,
            refresh_devices,
            connect_device,
            select_device,
            capture_screen,
            tap_screen
        ])
        .run(tauri::generate_context!())
        .expect("failed to run OnmyojiSupportTools");
}
