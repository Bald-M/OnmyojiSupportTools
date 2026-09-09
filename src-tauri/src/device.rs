use std::{
    collections::HashSet,
    env, fs,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::{process::Command, sync::RwLock, time::timeout};

const ADB_TIMEOUT: Duration = Duration::from_secs(8);
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdbCandidate {
    pub path: String,
    pub source: AdbSource,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AdbSource {
    Saved,
    #[cfg(target_os = "windows")]
    MuMu12,
    #[cfg(target_os = "windows")]
    LdPlayer9,
    Path,
    Manual,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSummary {
    pub serial: String,
    pub model: Option<String>,
    pub status: DeviceStatus,
    pub transport: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceStatus {
    Online,
    Offline,
    Unauthorized,
    Unknown,
}

impl DeviceStatus {
    #[cfg(test)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Offline => "offline",
            Self::Unauthorized => "unauthorized",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectEndpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct Point {
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameSummary {
    pub width: u32,
    pub height: u32,
    pub device_serial: String,
    pub captured_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TapReceipt {
    pub device_serial: String,
    pub point: Point,
    pub completed_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub adb_candidates: Vec<AdbCandidate>,
    pub selected_adb: Option<AdbCandidate>,
    pub devices: Vec<DeviceSummary>,
    pub active_device_serial: Option<String>,
    pub last_frame: Option<FrameSummary>,
    pub last_endpoint: Option<ConnectEndpoint>,
}

#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
    pub recovery: Option<String>,
}

impl AppError {
    fn new(code: &'static str, message: impl Into<String>, recovery: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            recovery: Some(recovery.into()),
        }
    }
}

#[derive(Debug)]
pub(crate) struct CommandOutput {
    success: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[async_trait]
pub(crate) trait CommandRunner: Send + Sync {
    async fn run(
        &self,
        program: &Path,
        args: &[String],
        limit: Duration,
    ) -> Result<CommandOutput, AppError>;
}

struct ProcessRunner;

#[async_trait]
impl CommandRunner for ProcessRunner {
    async fn run(
        &self,
        program: &Path,
        args: &[String],
        limit: Duration,
    ) -> Result<CommandOutput, AppError> {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let result = timeout(limit, command.output())
            .await
            .map_err(|_| {
                AppError::new(
                    "ADB_TIMEOUT",
                    "ADB 操作超时",
                    "请确认模拟器仍在运行，然后重试",
                )
            })?
            .map_err(|error| {
                AppError::new(
                    "ADB_NOT_FOUND",
                    format!("无法启动 ADB：{error}"),
                    "请重新选择有效的 adb.exe",
                )
            })?;

        Ok(CommandOutput {
            success: result.status.success(),
            stdout: result.stdout,
            stderr: result.stderr,
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredConfig {
    adb_path: Option<PathBuf>,
    active_device_serial: Option<String>,
    last_endpoint: Option<ConnectEndpoint>,
}

#[derive(Default)]
struct RuntimeState {
    initialized: bool,
    adb_candidates: Vec<AdbCandidate>,
    selected_adb: Option<AdbCandidate>,
    devices: Vec<DeviceSummary>,
    active_device_serial: Option<String>,
    last_frame: Option<FrameSummary>,
    config: StoredConfig,
}

pub struct DeviceManager {
    runner: Arc<dyn CommandRunner>,
    config_path: PathBuf,
    state: RwLock<RuntimeState>,
}

impl DeviceManager {
    pub fn new(config_path: PathBuf) -> Self {
        Self::with_runner(config_path, Arc::new(ProcessRunner))
    }

    fn with_runner(config_path: PathBuf, runner: Arc<dyn CommandRunner>) -> Self {
        Self {
            runner,
            config_path,
            state: RwLock::new(RuntimeState::default()),
        }
    }

    pub async fn initialize(&self) -> Result<AppState, AppError> {
        if self.state.read().await.initialized {
            return Ok(self.snapshot().await);
        }

        let config = self.load_config()?;
        let mut candidates = Vec::new();
        for (path, source) in discover_adb_paths(config.adb_path.as_deref()) {
            if let Ok(candidate) = self.validate_adb(&path, source).await {
                candidates.push(candidate);
            }
        }

        let selected_adb = select_initial_adb(&candidates, config.adb_path.as_deref());

        {
            let mut state = self.state.write().await;
            state.initialized = true;
            state.adb_candidates = candidates;
            state.selected_adb = selected_adb;
            state.config = config;
        }

        if self.state.read().await.selected_adb.is_some() {
            self.refresh_devices().await
        } else {
            Ok(self.snapshot().await)
        }
    }

    pub async fn set_adb_path(&self, path: PathBuf) -> Result<AppState, AppError> {
        let source = self
            .state
            .read()
            .await
            .adb_candidates
            .iter()
            .find(|candidate| same_path(Path::new(&candidate.path), &path))
            .map(|candidate| candidate.source)
            .unwrap_or(AdbSource::Manual);
        let candidate = self.validate_adb(&path, source).await?;
        {
            let mut state = self.state.write().await;
            state.initialized = true;
            state
                .adb_candidates
                .retain(|item| !same_path(Path::new(&item.path), &path));
            state.adb_candidates.push(candidate.clone());
            state.selected_adb = Some(candidate);
            state.devices.clear();
            state.active_device_serial = None;
            state.last_frame = None;
            state.config.adb_path = Some(path);
            state.config.active_device_serial = None;
            self.save_config(&state.config)?;
        }

        self.refresh_devices().await
    }

    pub async fn refresh_devices(&self) -> Result<AppState, AppError> {
        let adb = self.selected_adb_path().await?;
        let output = self
            .runner
            .run(&adb, &strings(&["devices", "-l"]), ADB_TIMEOUT)
            .await?;
        ensure_success(
            output.success,
            &output.stdout,
            &output.stderr,
            "DEVICE_LIST_FAILED",
            "无法读取设备列表",
            "请检查 ADB 和模拟器状态后重试",
        )?;
        let devices = parse_devices(&String::from_utf8_lossy(&output.stdout));

        let mut state = self.state.write().await;
        let previous = state.active_device_serial.clone();
        let saved = state.config.active_device_serial.clone();
        let online: Vec<_> = devices
            .iter()
            .filter(|device| device.status == DeviceStatus::Online)
            .map(|device| device.serial.clone())
            .collect();
        let active = previous
            .filter(|serial| online.contains(serial))
            .or_else(|| saved.filter(|serial| online.contains(serial)))
            .or_else(|| (online.len() == 1).then(|| online[0].clone()));

        if active != state.active_device_serial {
            state.last_frame = None;
        }
        state.devices = devices;
        state.active_device_serial = active.clone();
        state.config.active_device_serial = active;
        self.save_config(&state.config)?;
        Ok(snapshot_from(&state))
    }

    pub async fn connect_device(&self, endpoint: ConnectEndpoint) -> Result<AppState, AppError> {
        let target = parse_endpoint(&endpoint)?;
        let adb = self.selected_adb_path().await?;
        let output = self
            .runner
            .run(&adb, &strings(&["connect", &target]), ADB_TIMEOUT)
            .await?;
        ensure_success(
            output.success,
            &output.stdout,
            &output.stderr,
            "DEVICE_CONNECT_FAILED",
            "连接模拟器失败",
            "请检查 IP、端口和模拟器的 ADB 设置",
        )?;
        ensure_connect_response(&output.stdout)?;

        {
            let mut state = self.state.write().await;
            state.config.last_endpoint = Some(endpoint);
            self.save_config(&state.config)?;
        }
        self.refresh_devices().await
    }

    pub async fn select_device(&self, serial: String) -> Result<AppState, AppError> {
        let mut state = self.state.write().await;
        let available = state
            .devices
            .iter()
            .any(|device| device.serial == serial && device.status == DeviceStatus::Online);
        if !available {
            return Err(AppError::new(
                "DEVICE_NOT_READY",
                "所选设备不在线",
                "请刷新设备列表并选择在线设备",
            ));
        }

        if state.active_device_serial.as_deref() != Some(&serial) {
            state.last_frame = None;
        }
        state.active_device_serial = Some(serial.clone());
        state.config.active_device_serial = Some(serial);
        self.save_config(&state.config)?;
        Ok(snapshot_from(&state))
    }

    pub async fn capture_screen(&self) -> Result<Vec<u8>, AppError> {
        let (adb, serial) = self.active_command_context().await?;
        let output = self
            .runner
            .run(
                &adb,
                &strings(&["-s", &serial, "exec-out", "screencap", "-p"]),
                ADB_TIMEOUT,
            )
            .await?;
        ensure_success(
            output.success,
            &output.stdout,
            &output.stderr,
            "CAPTURE_FAILED",
            "截图失败",
            "请确认设备已解锁并保持在线",
        )?;
        let (width, height) = validate_png(&output.stdout)?;

        let mut state = self.state.write().await;
        if state.active_device_serial.as_deref() != Some(&serial) {
            return Err(AppError::new(
                "DEVICE_CHANGED",
                "截图期间活动设备已切换",
                "请重新获取当前设备截图",
            ));
        }
        state.last_frame = Some(FrameSummary {
            width,
            height,
            device_serial: serial,
            captured_at: epoch_millis(),
        });
        Ok(output.stdout)
    }

    pub async fn tap_screen(&self, point: Point) -> Result<TapReceipt, AppError> {
        let (adb, serial, frame) = {
            let state = self.state.read().await;
            let adb = selected_path(&state)?;
            let serial = state.active_device_serial.clone().ok_or_else(|| {
                AppError::new("DEVICE_NOT_FOUND", "尚未选择活动设备", "请先选择在线设备")
            })?;
            let frame = state
                .last_frame
                .clone()
                .filter(|frame| frame.device_serial == serial)
                .ok_or_else(|| {
                    AppError::new(
                        "FRAME_REQUIRED",
                        "点击前需要当前设备截图",
                        "请先刷新截图并选择坐标",
                    )
                })?;
            (adb, serial, frame)
        };

        if point.x >= frame.width || point.y >= frame.height {
            return Err(AppError::new(
                "POINT_OUT_OF_BOUNDS",
                "坐标超出截图范围",
                "请在截图内重新选择坐标",
            ));
        }

        let x = point.x.to_string();
        let y = point.y.to_string();
        let output = self
            .runner
            .run(
                &adb,
                &strings(&["-s", &serial, "shell", "input", "tap", &x, &y]),
                ADB_TIMEOUT,
            )
            .await?;
        ensure_success(
            output.success,
            &output.stdout,
            &output.stderr,
            "TAP_FAILED",
            "设备点击失败",
            "请确认设备在线且允许 ADB 控制",
        )?;

        Ok(TapReceipt {
            device_serial: serial,
            point,
            completed_at: epoch_millis(),
        })
    }

    async fn validate_adb(&self, path: &Path, source: AdbSource) -> Result<AdbCandidate, AppError> {
        if !path.is_file() {
            return Err(AppError::new(
                "ADB_INVALID",
                "所选路径不是 adb 可执行文件",
                "请选择 MuMu、雷电或 Android Platform-Tools 中的 adb.exe",
            ));
        }

        let output = self
            .runner
            .run(path, &strings(&["version"]), ADB_TIMEOUT)
            .await?;
        ensure_success(
            output.success,
            &output.stdout,
            &output.stderr,
            "ADB_INVALID",
            "无法验证所选 ADB",
            "请选择可正常运行的 adb.exe",
        )?;
        let text = String::from_utf8_lossy(&output.stdout);
        let version = parse_adb_version(&text).ok_or_else(|| {
            AppError::new(
                "ADB_INVALID",
                "程序输出不符合 ADB 版本格式",
                "请选择 Android Debug Bridge 可执行文件",
            )
        })?;

        Ok(AdbCandidate {
            path: path.to_string_lossy().into_owned(),
            source,
            version,
        })
    }

    async fn selected_adb_path(&self) -> Result<PathBuf, AppError> {
        let state = self.state.read().await;
        selected_path(&state)
    }

    async fn active_command_context(&self) -> Result<(PathBuf, String), AppError> {
        let state = self.state.read().await;
        let adb = selected_path(&state)?;
        let serial = state.active_device_serial.clone().ok_or_else(|| {
            AppError::new(
                "DEVICE_NOT_FOUND",
                "尚未选择设备",
                "请刷新设备列表并选择在线设备",
            )
        })?;
        Ok((adb, serial))
    }

    async fn snapshot(&self) -> AppState {
        let state = self.state.read().await;
        snapshot_from(&state)
    }

    fn load_config(&self) -> Result<StoredConfig, AppError> {
        match fs::read(&self.config_path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                AppError::new(
                    "CONFIG_INVALID",
                    format!("应用配置无法读取：{error}"),
                    "请移除损坏的 config.json 后重新启动应用",
                )
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(StoredConfig::default())
            }
            Err(error) => Err(AppError::new(
                "CONFIG_FAILED",
                format!("无法读取应用配置：{error}"),
                "请检查应用配置目录权限",
            )),
        }
    }

    fn save_config(&self, config: &StoredConfig) -> Result<(), AppError> {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent).map_err(config_write_error)?;
        }
        let bytes = serde_json::to_vec_pretty(config).map_err(config_write_error)?;
        fs::write(&self.config_path, bytes).map_err(config_write_error)
    }
}

fn selected_path(state: &RuntimeState) -> Result<PathBuf, AppError> {
    state
        .selected_adb
        .as_ref()
        .map(|candidate| PathBuf::from(&candidate.path))
        .ok_or_else(|| {
            AppError::new(
                "ADB_NOT_FOUND",
                "尚未选择 ADB",
                "请选择检测到的 ADB，或手动定位 adb.exe",
            )
        })
}

fn snapshot_from(state: &RuntimeState) -> AppState {
    AppState {
        adb_candidates: state.adb_candidates.clone(),
        selected_adb: state.selected_adb.clone(),
        devices: state.devices.clone(),
        active_device_serial: state.active_device_serial.clone(),
        last_frame: state.last_frame.clone(),
        last_endpoint: state.config.last_endpoint.clone(),
    }
}

fn config_write_error(error: impl std::fmt::Display) -> AppError {
    AppError::new(
        "CONFIG_FAILED",
        format!("无法保存应用配置：{error}"),
        "请检查应用配置目录是否可写",
    )
}

fn ensure_success(
    success: bool,
    stdout: &[u8],
    stderr: &[u8],
    code: &'static str,
    message: &'static str,
    recovery: &'static str,
) -> Result<(), AppError> {
    if success {
        return Ok(());
    }

    let detail = if stderr.is_empty() { stdout } else { stderr };
    let detail = String::from_utf8_lossy(detail);
    let detail = detail.trim();
    let message = if detail.is_empty() {
        message.to_owned()
    } else {
        format!(
            "{message}：{}",
            detail.chars().take(240).collect::<String>()
        )
    };
    Err(AppError::new(code, message, recovery))
}

fn ensure_connect_response(stdout: &[u8]) -> Result<(), AppError> {
    let response = String::from_utf8_lossy(stdout);
    let failed = response.lines().any(|line| {
        let line = line.trim().to_ascii_lowercase();
        line.starts_with("failed to connect") || line.starts_with("cannot connect")
    });
    if !failed {
        return Ok(());
    }

    Err(AppError::new(
        "DEVICE_CONNECT_FAILED",
        format!(
            "连接模拟器失败：{}",
            response.trim().chars().take(240).collect::<String>()
        ),
        "请检查 IP、端口和模拟器的 ADB 设置",
    ))
}

fn parse_endpoint(endpoint: &ConnectEndpoint) -> Result<String, AppError> {
    let ip = endpoint.host.trim().parse::<IpAddr>().map_err(|_| {
        AppError::new(
            "INVALID_ENDPOINT",
            "IP 地址格式无效",
            "请输入 IPv4 或 IPv6 地址，例如 127.0.0.1",
        )
    })?;
    if endpoint.port == 0 {
        return Err(AppError::new(
            "INVALID_ENDPOINT",
            "端口必须大于 0",
            "请输入 1 到 65535 之间的端口",
        ));
    }
    Ok(SocketAddr::new(ip, endpoint.port).to_string())
}

fn validate_png(bytes: &[u8]) -> Result<(u32, u32), AppError> {
    if bytes.len() < 24 || &bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return Err(AppError::new(
            "INVALID_FRAME",
            "ADB 返回的截图不是有效 PNG",
            "请确认模拟器已启动并解锁，然后重试截图",
        ));
    }

    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("checked PNG header length"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("checked PNG header length"));
    if width == 0 || height == 0 {
        return Err(AppError::new(
            "INVALID_FRAME",
            "截图尺寸无效",
            "请确认模拟器画面可正常显示",
        ));
    }
    Ok((width, height))
}

fn parse_adb_version(output: &str) -> Option<String> {
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix("Version ").map(str::to_owned))
        .or_else(|| {
            output.lines().find_map(|line| {
                line.trim()
                    .strip_prefix("Android Debug Bridge version ")
                    .map(str::to_owned)
            })
        })
}

fn parse_devices(output: &str) -> Vec<DeviceSummary> {
    output
        .lines()
        .skip_while(|line| !line.starts_with("List of devices attached"))
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let serial = fields.next()?;
            let raw_status = fields.next()?;
            let status = match raw_status {
                "device" => DeviceStatus::Online,
                "offline" => DeviceStatus::Offline,
                "unauthorized" => DeviceStatus::Unauthorized,
                _ => DeviceStatus::Unknown,
            };
            let mut model = None;
            let mut transport = None;

            for field in fields {
                if let Some(value) = field.strip_prefix("model:") {
                    model = Some(value.replace('_', " "));
                } else if let Some(value) = field.strip_prefix("transport_id:") {
                    transport = Some(value.to_owned());
                }
            }

            Some(DeviceSummary {
                serial: serial.to_owned(),
                model,
                status,
                transport,
            })
        })
        .collect()
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn find_candidate(candidates: &[AdbCandidate], path: &Path) -> Option<AdbCandidate> {
    candidates
        .iter()
        .find(|candidate| same_path(Path::new(&candidate.path), path))
        .cloned()
}

fn select_initial_adb(candidates: &[AdbCandidate], saved: Option<&Path>) -> Option<AdbCandidate> {
    saved
        .and_then(|path| find_candidate(candidates, path))
        .or_else(|| (candidates.len() == 1).then(|| candidates[0].clone()))
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn discover_adb_paths(saved: Option<&Path>) -> Vec<(PathBuf, AdbSource)> {
    let mut candidates = Vec::new();
    if let Some(path) = saved {
        candidates.push((path.to_path_buf(), AdbSource::Saved));
    }
    if let Some(path) = find_on_path() {
        candidates.push((path, AdbSource::Path));
    }
    candidates.extend(platform_adb_paths());

    let mut seen = HashSet::new();
    candidates.retain(|(path, _)| {
        path.is_file() && seen.insert(path.canonicalize().unwrap_or_else(|_| path.clone()))
    });
    candidates
}

fn find_on_path() -> Option<PathBuf> {
    let executable = if cfg!(windows) { "adb.exe" } else { "adb" };
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|directory| directory.join(executable))
            .find(|path| path.is_file())
    })
}

#[cfg(target_os = "windows")]
fn platform_adb_paths() -> Vec<(PathBuf, AdbSource)> {
    const KNOWN: &[(&str, AdbSource)] = &[
        (
            r"Program Files\Netease\MuMuPlayer-12.0\shell\adb.exe",
            AdbSource::MuMu12,
        ),
        (
            r"Program Files\Netease\MuMuPlayerGlobal-12.0\shell\adb.exe",
            AdbSource::MuMu12,
        ),
        (
            r"Program Files\NetEase\MuMuPlayer-12.0\shell\adb.exe",
            AdbSource::MuMu12,
        ),
        (
            r"Program Files\leidian\LDPlayer9\adb.exe",
            AdbSource::LdPlayer9,
        ),
        (
            r"Program Files\ChangZhi\LDPlayer\adb.exe",
            AdbSource::LdPlayer9,
        ),
        (r"LDPlayer\LDPlayer9\adb.exe", AdbSource::LdPlayer9),
    ];

    ('C'..='Z')
        .flat_map(|drive| {
            KNOWN.iter().map(move |(relative, source)| {
                (PathBuf::from(format!("{drive}:\\{relative}")), *source)
            })
        })
        .collect()
}

#[cfg(not(target_os = "windows"))]
fn platform_adb_paths() -> Vec<(PathBuf, AdbSource)> {
    Vec::new()
}

fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, path::Path, sync::Arc, time::Duration};

    use async_trait::async_trait;
    use tokio::sync::Mutex;

    use super::{
        AdbCandidate, AdbSource, AppError, CommandOutput, CommandRunner, ConnectEndpoint,
        DeviceManager, Point, ensure_success, parse_devices, parse_endpoint, select_initial_adb,
        strings, validate_png,
    };

    struct FakeRunner {
        outputs: Mutex<VecDeque<Result<CommandOutput, AppError>>>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl FakeRunner {
        fn with_outputs(outputs: Vec<CommandOutput>) -> Self {
            Self::with_results(outputs.into_iter().map(Ok).collect())
        }

        fn with_results(outputs: Vec<Result<CommandOutput, AppError>>) -> Self {
            Self {
                outputs: Mutex::new(outputs.into()),
                calls: Mutex::new(Vec::new()),
            }
        }

        async fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().await.clone()
        }
    }

    #[async_trait]
    impl CommandRunner for FakeRunner {
        async fn run(
            &self,
            _program: &Path,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, AppError> {
            self.calls.lock().await.push(args.to_vec());
            self.outputs
                .lock()
                .await
                .pop_front()
                .expect("queued output")
        }
    }

    fn success(stdout: &str) -> CommandOutput {
        CommandOutput {
            success: true,
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    fn success_bytes(stdout: &[u8]) -> CommandOutput {
        CommandOutput {
            success: true,
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        }
    }

    #[test]
    fn parses_devices_with_online_and_unavailable_states() {
        let output = "List of devices attached\n127.0.0.1:16384 device product:MuMu model:MuMu_12 transport_id:1\nemulator-5554 offline transport_id:2\nusb-device unauthorized usb:1-2 transport_id:3\n";

        let devices = parse_devices(output);

        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].serial, "127.0.0.1:16384");
        assert_eq!(devices[0].model.as_deref(), Some("MuMu 12"));
        assert_eq!(devices[0].status.as_str(), "online");
        assert_eq!(devices[1].status.as_str(), "offline");
        assert_eq!(devices[2].status.as_str(), "unauthorized");
    }

    #[test]
    fn accepts_ip_endpoints_and_rejects_command_text() {
        let endpoint = ConnectEndpoint {
            host: "127.0.0.1".to_owned(),
            port: 16384,
        };
        assert_eq!(parse_endpoint(&endpoint).unwrap(), "127.0.0.1:16384");

        let invalid = ConnectEndpoint {
            host: "127.0.0.1 && calc.exe".to_owned(),
            port: 16384,
        };
        assert_eq!(
            parse_endpoint(&invalid).unwrap_err().code,
            "INVALID_ENDPOINT"
        );
    }

    #[test]
    fn reads_dimensions_from_a_png_header() {
        let png = [
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0,
            0, 5, 0, 0, 0, 2, 208,
        ];

        assert_eq!(validate_png(&png).unwrap(), (1280, 720));
        assert_eq!(
            validate_png(b"not a png").unwrap_err().code,
            "INVALID_FRAME"
        );
    }

    #[test]
    fn maps_process_failures_to_a_stable_recoverable_error() {
        let error = ensure_success(
            false,
            b"",
            b"device offline",
            "DEVICE_LIST_FAILED",
            "无法读取设备列表",
            "请刷新设备",
        )
        .unwrap_err();

        assert_eq!(error.code, "DEVICE_LIST_FAILED");
        assert!(error.message.contains("device offline"));
        assert_eq!(error.recovery.as_deref(), Some("请刷新设备"));
    }

    #[test]
    fn selects_a_saved_or_only_adb_candidate_but_not_an_ambiguous_one() {
        let first = AdbCandidate {
            path: "first-adb.exe".to_owned(),
            source: AdbSource::Manual,
            version: "35.0.1".to_owned(),
        };
        let second = AdbCandidate {
            path: "second-adb.exe".to_owned(),
            source: AdbSource::Path,
            version: "35.0.2".to_owned(),
        };

        assert_eq!(
            select_initial_adb(std::slice::from_ref(&first), None)
                .unwrap()
                .path,
            first.path
        );
        assert!(select_initial_adb(&[first.clone(), second.clone()], None).is_none());
        assert_eq!(
            select_initial_adb(&[first, second.clone()], Some(Path::new("second-adb.exe")),)
                .unwrap()
                .path,
            second.path
        );
    }

    #[tokio::test]
    async fn rejects_an_invalid_adb_path_without_starting_a_process() {
        let directory = tempfile::tempdir().unwrap();
        let runner = Arc::new(FakeRunner::with_outputs(Vec::new()));
        let manager =
            DeviceManager::with_runner(directory.path().join("config.json"), runner.clone());

        let error = manager
            .set_adb_path(directory.path().join("missing-adb.exe"))
            .await
            .unwrap_err();

        assert_eq!(error.code, "ADB_INVALID");
        assert!(runner.calls().await.is_empty());
    }

    #[tokio::test]
    async fn preserves_timeout_errors_from_the_command_adapter() {
        let directory = tempfile::tempdir().unwrap();
        let adb_path = directory.path().join("adb.exe");
        std::fs::write(&adb_path, []).unwrap();
        let runner = Arc::new(FakeRunner::with_results(vec![Err(AppError::new(
            "ADB_TIMEOUT",
            "ADB 操作超时",
            "请重试",
        ))]));
        let manager = DeviceManager::with_runner(directory.path().join("config.json"), runner);

        assert_eq!(
            manager.set_adb_path(adb_path).await.unwrap_err().code,
            "ADB_TIMEOUT"
        );
    }

    #[tokio::test]
    async fn selecting_adb_refreshes_devices_and_selects_the_only_online_device() {
        let directory = tempfile::tempdir().unwrap();
        let adb_path = directory.path().join("adb.exe");
        std::fs::write(&adb_path, []).unwrap();
        let runner = Arc::new(FakeRunner::with_outputs(vec![
            success("Android Debug Bridge version 1.0.41\nVersion 35.0.2-12147458"),
            success(
                "List of devices attached\n127.0.0.1:16384 device model:MuMu_12 transport_id:1\n",
            ),
        ]));
        let manager = DeviceManager::with_runner(directory.path().join("config.json"), runner);

        let state = manager.set_adb_path(adb_path).await.unwrap();

        assert_eq!(state.selected_adb.unwrap().version, "35.0.2-12147458");
        assert_eq!(
            state.active_device_serial.as_deref(),
            Some("127.0.0.1:16384")
        );
    }

    #[tokio::test]
    async fn capture_is_required_and_bounds_are_checked_before_tapping() {
        let directory = tempfile::tempdir().unwrap();
        let adb_path = directory.path().join("adb.exe");
        std::fs::write(&adb_path, []).unwrap();
        let screenshot = [
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0,
            0, 5, 0, 0, 0, 2, 208,
        ];
        let runner = Arc::new(FakeRunner::with_outputs(vec![
            success("Android Debug Bridge version 1.0.41\nVersion 35.0.2"),
            success("List of devices attached\nemulator-5554 device model:LDPlayer_9\n"),
            success_bytes(&screenshot),
            success(""),
        ]));
        let manager =
            DeviceManager::with_runner(directory.path().join("config.json"), runner.clone());
        manager.set_adb_path(adb_path).await.unwrap();

        assert_eq!(
            manager
                .tap_screen(Point { x: 1, y: 1 })
                .await
                .unwrap_err()
                .code,
            "FRAME_REQUIRED"
        );
        assert_eq!(manager.capture_screen().await.unwrap(), screenshot);
        assert_eq!(
            manager
                .tap_screen(Point { x: 1280, y: 1 })
                .await
                .unwrap_err()
                .code,
            "POINT_OUT_OF_BOUNDS"
        );
        let receipt = manager.tap_screen(Point { x: 1279, y: 719 }).await.unwrap();
        assert_eq!(receipt.device_serial, "emulator-5554");
        assert_eq!(receipt.point.x, 1279);
        assert_eq!(
            runner.calls().await[2],
            strings(&["-s", "emulator-5554", "exec-out", "screencap", "-p"])
        );
        assert_eq!(
            runner.calls().await[3],
            strings(&[
                "-s",
                "emulator-5554",
                "shell",
                "input",
                "tap",
                "1279",
                "719"
            ])
        );
    }

    #[tokio::test]
    async fn switching_devices_invalidates_the_previous_frame() {
        let directory = tempfile::tempdir().unwrap();
        let adb_path = directory.path().join("adb.exe");
        std::fs::write(&adb_path, []).unwrap();
        let screenshot = [
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0,
            0, 0, 2, 0, 0, 0, 1, 0,
        ];
        let runner = Arc::new(FakeRunner::with_outputs(vec![
            success("Android Debug Bridge version 1.0.41\nVersion 35.0.2"),
            success(
                "List of devices attached\nfirst device model:MuMu_12\nsecond device model:LDPlayer_9\n",
            ),
            success_bytes(&screenshot),
        ]));
        let manager = DeviceManager::with_runner(directory.path().join("config.json"), runner);
        manager.set_adb_path(adb_path).await.unwrap();
        manager.select_device("first".to_owned()).await.unwrap();
        manager.capture_screen().await.unwrap();
        manager.select_device("second".to_owned()).await.unwrap();

        assert_eq!(
            manager
                .tap_screen(Point { x: 10, y: 10 })
                .await
                .unwrap_err()
                .code,
            "FRAME_REQUIRED"
        );
    }

    #[tokio::test]
    async fn treats_adb_connect_failure_text_as_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let adb_path = directory.path().join("adb.exe");
        std::fs::write(&adb_path, []).unwrap();
        let runner = Arc::new(FakeRunner::with_outputs(vec![
            success("Android Debug Bridge version 1.0.41\nVersion 35.0.2"),
            success("List of devices attached\n"),
            success("failed to connect to 127.0.0.1:16384: cannot connect"),
        ]));
        let manager = DeviceManager::with_runner(directory.path().join("config.json"), runner);
        manager.set_adb_path(adb_path).await.unwrap();

        assert_eq!(
            manager
                .connect_device(ConnectEndpoint {
                    host: "127.0.0.1".to_owned(),
                    port: 16384,
                })
                .await
                .unwrap_err()
                .code,
            "DEVICE_CONNECT_FAILED"
        );
    }
}
