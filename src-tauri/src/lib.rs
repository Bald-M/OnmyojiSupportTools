mod activity;
mod click;
mod device;
mod preview;

use std::path::PathBuf;

use activity::{ActivityConfig, ActivitySession, RecognitionResult, Rect, VisualFeature};
use click::ClickTarget;
use device::{AppError, AppState, ClickSettings, ConnectEndpoint, DeviceManager, TapReceipt};
use std::sync::Arc;

use tauri::{
    Manager, State,
    ipc::{Channel, InvokeResponseBody, Response},
};

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
    target: ClickTarget,
    manager: State<'_, DeviceManager>,
) -> Result<TapReceipt, AppError> {
    manager.tap_screen(target).await
}

#[tauri::command]
async fn set_click_settings(
    settings: ClickSettings,
    manager: State<'_, DeviceManager>,
) -> Result<AppState, AppError> {
    manager.set_click_settings(settings).await
}

#[tauri::command]
async fn cancel_pending_click(manager: State<'_, DeviceManager>) -> Result<(), AppError> {
    manager.cancel_pending_click().await;
    Ok(())
}

#[tauri::command]
async fn save_activity_config(
    config: ActivityConfig,
    manager: State<'_, DeviceManager>,
) -> Result<Vec<ActivityConfig>, AppError> {
    manager.save_activity_config(config).await
}

#[tauri::command]
async fn delete_activity_config(
    id: String,
    manager: State<'_, DeviceManager>,
) -> Result<Vec<ActivityConfig>, AppError> {
    manager.delete_activity_config(&id).await
}

#[tauri::command]
async fn calibrate_activity_feature(
    region: Rect,
    manager: State<'_, DeviceManager>,
) -> Result<VisualFeature, AppError> {
    manager.calibrate_activity_feature(region).await
}

#[tauri::command]
async fn preview_activity_recognition(
    config_id: String,
    manager: State<'_, DeviceManager>,
) -> Result<RecognitionResult, AppError> {
    manager.preview_activity_recognition(&config_id).await
}

#[tauri::command]
async fn start_activity(
    config_id: String,
    target_runs: u32,
    manager: State<'_, DeviceManager>,
) -> Result<ActivitySession, AppError> {
    manager.start_activity(config_id, target_runs).await
}

#[tauri::command]
async fn pause_activity(manager: State<'_, DeviceManager>) -> Result<ActivitySession, AppError> {
    Ok(manager.pause_activity("用户暂停").await)
}

#[tauri::command]
async fn resume_activity(manager: State<'_, DeviceManager>) -> Result<ActivitySession, AppError> {
    manager.resume_activity().await
}

#[tauri::command]
async fn stop_activity(manager: State<'_, DeviceManager>) -> Result<ActivitySession, AppError> {
    Ok(manager.stop_activity().await)
}

#[tauri::command]
async fn advance_activity(manager: State<'_, DeviceManager>) -> Result<ActivitySession, AppError> {
    manager.advance_activity().await
}

#[tauri::command]
async fn start_preview(
    on_chunk: Channel<InvokeResponseBody>,
    on_ended: Channel<String>,
    manager: State<'_, DeviceManager>,
) -> Result<AppState, AppError> {
    manager
        .start_preview(
            Arc::new(move |bytes| {
                let _ = on_chunk.send(InvokeResponseBody::Raw(bytes));
            }),
            Arc::new(move || {
                let _ = on_ended.send("ended".to_owned());
            }),
        )
        .await
}

#[tauri::command]
async fn stop_preview(manager: State<'_, DeviceManager>) -> Result<AppState, AppError> {
    manager.stop_preview().await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let config_path = app.path().app_config_dir()?.join("config.json");
            let bundled_adb_path = if cfg!(target_os = "windows") {
                Some(app.path().resource_dir()?.join("adb").join("adb.exe"))
            } else {
                None
            };
            app.manage(DeviceManager::new(config_path, bundled_adb_path));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_app_state,
            set_adb_path,
            refresh_devices,
            connect_device,
            select_device,
            capture_screen,
            tap_screen,
            set_click_settings,
            cancel_pending_click,
            save_activity_config,
            delete_activity_config,
            calibrate_activity_feature,
            preview_activity_recognition,
            start_activity,
            pause_activity,
            resume_activity,
            stop_activity,
            advance_activity,
            start_preview,
            stop_preview
        ])
        .build(tauri::generate_context!())
        .expect("failed to build OnmyojiSupportTools");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event
            && let Err(error) = tauri::async_runtime::block_on(async {
                let manager = app_handle.state::<DeviceManager>();
                manager.shutdown().await
            })
        {
            eprintln!("failed to stop the bundled ADB server: {error}");
        }
    });
}
