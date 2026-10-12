mod activity;
mod click;
mod device;
mod diagnostics;
mod emulator_discovery;
mod preview;
mod process;

use std::path::PathBuf;

use activity::{ActivityConfig, ActivitySession, RecognitionResult, Rect, VisualFeature};
use click::ClickTarget;
use device::{AppError, AppState, ClickSettings, ConnectEndpoint, DeviceManager, TapReceipt};
use preview::PreviewEnd;
use std::sync::Arc;

use tauri::{
    Manager, State,
    ipc::{Channel, InvokeResponseBody, Response},
};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
async fn get_app_app_state(
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<AppState, AppError> {
    diagnostics::operation("initialize", trace_id, manager.initialize()).await
}

#[tauri::command]
async fn set_adb_path(
    path: String,
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<AppState, AppError> {
    diagnostics::operation(
        "set_adb_path",
        trace_id,
        manager.set_adb_path(PathBuf::from(path)),
    )
    .await
}

#[tauri::command]
async fn refresh_devices(
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<AppState, AppError> {
    diagnostics::operation("refresh_devices", trace_id, manager.refresh_devices()).await
}

#[tauri::command]
async fn connect_device(
    endpoint: ConnectEndpoint,
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<AppState, AppError> {
    diagnostics::operation("connect_device", trace_id, manager.connect_device(endpoint)).await
}

#[tauri::command]
async fn select_device(
    serial: String,
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<AppState, AppError> {
    diagnostics::operation("select_device", trace_id, manager.select_device(serial)).await
}

#[tauri::command]
async fn capture_screen(
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<Response, AppError> {
    diagnostics::operation("capture_screen", trace_id, async {
        manager.capture_screen().await.map(Response::new)
    })
    .await
}

#[tauri::command]
async fn tap_screen(
    target: ClickTarget,
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<TapReceipt, AppError> {
    diagnostics::operation("tap_screen", trace_id, manager.tap_screen(target)).await
}

#[tauri::command]
async fn set_click_settings(
    settings: ClickSettings,
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<AppState, AppError> {
    diagnostics::operation(
        "set_click_settings",
        trace_id,
        manager.set_click_settings(settings),
    )
    .await
}

#[tauri::command]
async fn cancel_pending_click(
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<(), AppError> {
    diagnostics::operation("cancel_pending_click", trace_id, async {
        manager.cancel_pending_click().await;
        Ok(())
    })
    .await
}

#[tauri::command]
async fn save_activity_config(
    config: ActivityConfig,
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<Vec<ActivityConfig>, AppError> {
    diagnostics::operation(
        "save_activity_config",
        trace_id,
        manager.save_activity_config(config),
    )
    .await
}

#[tauri::command]
async fn delete_activity_config(
    id: String,
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<Vec<ActivityConfig>, AppError> {
    diagnostics::operation(
        "delete_activity_config",
        trace_id,
        manager.delete_activity_config(&id),
    )
    .await
}

#[tauri::command]
async fn calibrate_activity_feature(
    region: Rect,
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<VisualFeature, AppError> {
    diagnostics::operation(
        "calibrate_activity_feature",
        trace_id,
        manager.calibrate_activity_feature(region),
    )
    .await
}

#[tauri::command]
async fn preview_activity_recognition(
    config_id: String,
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<RecognitionResult, AppError> {
    diagnostics::operation(
        "preview_activity_recognition",
        trace_id,
        manager.preview_activity_recognition(&config_id),
    )
    .await
}

#[tauri::command]
async fn start_activity(
    config_id: String,
    target_runs: u32,
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<ActivitySession, AppError> {
    diagnostics::operation(
        "start_activity",
        trace_id,
        manager.start_activity(config_id, target_runs),
    )
    .await
}

#[tauri::command]
async fn pause_activity(
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<ActivitySession, AppError> {
    diagnostics::operation("pause_activity", trace_id, async {
        Ok(manager.pause_activity("用户暂停").await)
    })
    .await
}

#[tauri::command]
async fn resume_activity(
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<ActivitySession, AppError> {
    diagnostics::operation("resume_activity", trace_id, manager.resume_activity()).await
}

#[tauri::command]
async fn stop_activity(
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<ActivitySession, AppError> {
    diagnostics::operation("stop_activity", trace_id, async {
        Ok(manager.stop_activity().await)
    })
    .await
}

#[tauri::command]
async fn advance_activity(
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<ActivitySession, AppError> {
    diagnostics::operation("advance_activity", trace_id, manager.advance_activity()).await
}

#[tauri::command]
async fn start_preview(
    on_chunk: Channel<InvokeResponseBody>,
    on_ended: Channel<PreviewEnd>,
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<AppState, AppError> {
    diagnostics::operation(
        "start_preview",
        trace_id,
        manager.start_preview(
            Arc::new(move |bytes| {
                let _ = on_chunk.send(InvokeResponseBody::Raw(bytes));
            }),
            Arc::new(move |end| {
                let _ = on_ended.send(end);
            }),
        ),
    )
    .await
}

#[tauri::command]
async fn stop_preview(
    trace_id: Option<String>,
    manager: State<'_, DeviceManager>,
) -> Result<AppState, AppError> {
    diagnostics::operation("stop_preview", trace_id, manager.stop_preview()).await
}

#[tauri::command]
fn diagnostics_status() -> bool {
    diagnostics::status()
}

#[tauri::command]
fn report_frontend_error(trace_id: String, code: String) {
    diagnostics::frontend_event(trace_id, &code);
}

#[tauri::command]
fn open_diagnostics_directory(app: tauri::AppHandle) -> Result<(), AppError> {
    app.opener()
        .open_path(
            diagnostics::directory()?.to_string_lossy().into_owned(),
            None::<&str>,
        )
        .map_err(|_| {
            AppError::new(
                "DIAGNOSTICS_OPEN_FAILED",
                "无法打开日志目录",
                "请检查桌面文件管理器后重试",
            )
        })
}

#[tauri::command]
fn export_diagnostics(path: String) -> Result<(), AppError> {
    diagnostics::export(std::path::Path::new(&path))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            if let Ok(log_dir) = app.path().app_log_dir() {
                diagnostics::initialize(log_dir);
            }
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
            stop_preview,
            diagnostics_status,
            report_frontend_error,
            open_diagnostics_directory,
            export_diagnostics
        ])
        .build(tauri::generate_context!())
        .expect("failed to build OnmyojiSupportTools");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            let result = tauri::async_runtime::block_on(async {
                let manager = app_handle.state::<DeviceManager>();
                diagnostics::operation("shutdown", None, manager.shutdown()).await
            });
            if let Err(error) = result {
                eprintln!("failed to stop the bundled ADB server: {}", error.code);
            }
        }
    });
}
