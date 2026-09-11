use std::{
    collections::HashSet,
    env, fs,
    net::{IpAddr, SocketAddr, TcpListener},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    process::Command,
    sync::{Mutex, RwLock},
    time::timeout,
};

const ADB_TIMEOUT: Duration = Duration::from_secs(8);
const BUNDLED_ADB_SERVER_PORT: u16 = 5038;
const BUNDLED_ADB_DISTRIBUTION_JSON: &str = include_str!("../adb-distribution.json");
const MAX_ADB_SERVER_STATUS_BYTES: usize = 1024 * 1024;
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
    Bundled,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct AdbServerIdentity {
    version: String,
    executable_absolute_path: PathBuf,
}

#[derive(Clone, PartialEq, prost::Message)]
struct AdbServerStatusProto {
    #[prost(string, tag = "5")]
    version: String,
    #[prost(string, tag = "6")]
    build: String,
    #[prost(string, tag = "7")]
    executable_absolute_path: String,
}

#[async_trait]
trait AdbServerProbe: Send + Sync {
    async fn identity(&self, port: u16, limit: Duration) -> Result<AdbServerIdentity, AppError>;
}

struct SmartSocketAdbServerProbe;

#[async_trait]
impl AdbServerProbe for SmartSocketAdbServerProbe {
    async fn identity(&self, port: u16, limit: Duration) -> Result<AdbServerIdentity, AppError> {
        query_adb_server_identity(port, limit).await
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredConfig {
    adb_path: Option<PathBuf>,
    active_device_serial: Option<String>,
    last_endpoint: Option<ConnectEndpoint>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundledAdbDistribution {
    version: String,
    files: Vec<BundledAdbFile>,
}

#[derive(Deserialize)]
struct BundledAdbFile {
    name: String,
    sha256: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BundledAdbIntegrity {
    Verify,
    #[cfg(test)]
    Skip,
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
    bundled_adb_path: Option<PathBuf>,
    bundled_adb_integrity: BundledAdbIntegrity,
    bundled_server_port: u16,
    bundled_server_owned: Mutex<bool>,
    server_probe: Arc<dyn AdbServerProbe>,
    state: RwLock<RuntimeState>,
}

impl DeviceManager {
    pub fn new(config_path: PathBuf, bundled_adb_path: Option<PathBuf>) -> Self {
        Self::with_runner_and_optional_bundled_adb(
            config_path,
            bundled_adb_path,
            BundledAdbIntegrity::Verify,
            BUNDLED_ADB_SERVER_PORT,
            Arc::new(ProcessRunner),
            Arc::new(SmartSocketAdbServerProbe),
        )
    }

    #[cfg(test)]
    fn with_runner(config_path: PathBuf, runner: Arc<dyn CommandRunner>) -> Self {
        Self::with_runner_and_optional_bundled_adb(
            config_path,
            None,
            BundledAdbIntegrity::Skip,
            BUNDLED_ADB_SERVER_PORT,
            runner,
            Arc::new(SmartSocketAdbServerProbe),
        )
    }

    #[cfg(test)]
    fn with_runner_and_bundled_adb(
        config_path: PathBuf,
        bundled_adb_path: PathBuf,
        runner: Arc<dyn CommandRunner>,
        server_probe: Arc<dyn AdbServerProbe>,
    ) -> Self {
        Self::with_runner_and_optional_bundled_adb(
            config_path,
            Some(bundled_adb_path),
            BundledAdbIntegrity::Skip,
            BUNDLED_ADB_SERVER_PORT,
            runner,
            server_probe,
        )
    }

    #[cfg(test)]
    fn with_runner_and_bundled_adb_on_port(
        config_path: PathBuf,
        bundled_adb_path: PathBuf,
        bundled_server_port: u16,
        runner: Arc<dyn CommandRunner>,
        server_probe: Arc<dyn AdbServerProbe>,
    ) -> Self {
        Self::with_runner_and_optional_bundled_adb(
            config_path,
            Some(bundled_adb_path),
            BundledAdbIntegrity::Skip,
            bundled_server_port,
            runner,
            server_probe,
        )
    }

    fn with_runner_and_optional_bundled_adb(
        config_path: PathBuf,
        bundled_adb_path: Option<PathBuf>,
        bundled_adb_integrity: BundledAdbIntegrity,
        bundled_server_port: u16,
        runner: Arc<dyn CommandRunner>,
        server_probe: Arc<dyn AdbServerProbe>,
    ) -> Self {
        Self {
            runner,
            config_path,
            bundled_adb_path,
            bundled_adb_integrity,
            bundled_server_port,
            bundled_server_owned: Mutex::new(false),
            server_probe,
            state: RwLock::new(RuntimeState::default()),
        }
    }

    pub async fn initialize(&self) -> Result<AppState, AppError> {
        if self.state.read().await.initialized {
            return Ok(self.snapshot().await);
        }

        let config = self.load_config()?;
        let mut candidates = Vec::new();
        let mut bundled_adb_error = None;
        for (path, source) in
            discover_adb_paths(config.adb_path.as_deref(), self.bundled_adb_path.as_deref())
        {
            match self.validate_adb(&path, source).await {
                Ok(candidate) => candidates.push(candidate),
                Err(error) if source == AdbSource::Bundled => {
                    bundled_adb_error = Some(AppError::new(
                        "BUNDLED_ADB_INVALID",
                        format!("应用内置 ADB 无法使用：{}", error.message),
                        "请重新安装应用，或手动选择一个有效的外部 adb.exe",
                    ));
                }
                Err(_) => {}
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
        } else if let Some(error) = bundled_adb_error {
            Err(error)
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
        self.ensure_bundled_server(&adb).await?;
        let args = self.adb_args(&adb, &["devices", "-l"]);
        let output = self.runner.run(&adb, &args, ADB_TIMEOUT).await?;
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
        self.ensure_bundled_server(&adb).await?;
        let args = self.adb_args(&adb, &["connect", &target]);
        let output = self.runner.run(&adb, &args, ADB_TIMEOUT).await?;
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
        self.ensure_bundled_server(&adb).await?;
        let args = self.adb_args(&adb, &["-s", &serial, "exec-out", "screencap", "-p"]);
        let output = self.runner.run(&adb, &args, ADB_TIMEOUT).await?;
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
        self.ensure_bundled_server(&adb).await?;
        let args = self.adb_args(&adb, &["-s", &serial, "shell", "input", "tap", &x, &y]);
        let output = self.runner.run(&adb, &args, ADB_TIMEOUT).await?;
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

    pub async fn shutdown(&self) -> Result<(), AppError> {
        let mut owned = self.bundled_server_owned.lock().await;
        if !*owned {
            return Ok(());
        }
        let Some(adb) = self.bundled_adb_path.as_deref() else {
            return Ok(());
        };
        if let Err(error) = self.verify_bundled_server_identity(adb).await {
            *owned = false;
            return Err(error);
        }

        let args = self.adb_args(adb, &["kill-server"]);
        let output = self.runner.run(adb, &args, ADB_TIMEOUT).await?;
        ensure_success(
            output.success,
            &output.stdout,
            &output.stderr,
            "ADB_SHUTDOWN_FAILED",
            "无法停止应用内置的 ADB 服务",
            "请关闭残留的 adb.exe 后再卸载或升级",
        )?;
        *owned = false;
        Ok(())
    }

    async fn validate_adb(&self, path: &Path, source: AdbSource) -> Result<AdbCandidate, AppError> {
        let bundled_distribution = if source == AdbSource::Bundled {
            let distribution = parse_bundled_adb_distribution()?;
            if self.bundled_adb_integrity == BundledAdbIntegrity::Verify {
                validate_bundled_adb_files(path, &distribution)?;
            }
            Some(distribution)
        } else {
            None
        };

        if !path.is_file() {
            return Err(AppError::new(
                "ADB_INVALID",
                "所选路径不是 adb 可执行文件",
                "请选择 MuMu、雷电或 Android Platform-Tools 中的 adb.exe",
            ));
        }

        let args = self.adb_args(path, &["version"]);
        let output = self.runner.run(path, &args, ADB_TIMEOUT).await?;
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
        if let Some(distribution) = bundled_distribution
            && version.split('-').next() != Some(distribution.version.as_str())
        {
            return Err(AppError::new(
                "ADB_INVALID",
                format!(
                    "应用内置 ADB 版本应为 {}，实际为 {version}",
                    distribution.version
                ),
                "请重新安装应用，或手动选择一个有效的外部 adb.exe",
            ));
        }

        Ok(AdbCandidate {
            path: path.to_string_lossy().into_owned(),
            source,
            version,
        })
    }

    fn adb_args(&self, path: &Path, args: &[&str]) -> Vec<String> {
        let mut values = Vec::with_capacity(args.len() + 2);
        if self.is_bundled_adb(path) {
            values.push("-P".to_owned());
            values.push(self.bundled_server_port.to_string());
        }
        values.extend(strings(args));
        values
    }

    fn is_bundled_adb(&self, path: &Path) -> bool {
        self.bundled_adb_path
            .as_deref()
            .is_some_and(|bundled| same_path(bundled, path))
    }

    async fn ensure_bundled_server(&self, path: &Path) -> Result<(), AppError> {
        if !self.is_bundled_adb(path) {
            return Ok(());
        }

        let mut owned = self.bundled_server_owned.lock().await;
        if *owned {
            return Ok(());
        }

        if let Err(port_error) = ensure_bundled_server_port_available(self.bundled_server_port) {
            if self.verify_bundled_server_identity(path).await.is_ok() {
                *owned = true;
                return Ok(());
            }
            return Err(port_error);
        }

        let args = self.adb_args(path, &["start-server"]);
        let output = self.runner.run(path, &args, ADB_TIMEOUT).await?;
        ensure_success(
            output.success,
            &output.stdout,
            &output.stderr,
            "BUNDLED_ADB_START_FAILED",
            "无法启动应用内置的 ADB 服务",
            "请重试，或选择一个外部 adb.exe",
        )?;
        self.verify_bundled_server_identity(path).await?;
        *owned = true;
        Ok(())
    }

    async fn verify_bundled_server_identity(&self, adb: &Path) -> Result<(), AppError> {
        let recovery = format!(
            "请关闭占用端口 {} 的程序后重试，或选择一个外部 adb.exe",
            self.bundled_server_port
        );
        let identity = self
            .server_probe
            .identity(self.bundled_server_port, ADB_TIMEOUT)
            .await
            .map_err(|error| {
                AppError::new(
                    "BUNDLED_ADB_SERVER_UNVERIFIED",
                    format!("无法确认应用内置 ADB 服务的身份：{}", error.message),
                    &recovery,
                )
            })?;
        let expected_version = parse_bundled_adb_distribution()?.version;
        if identity.version != expected_version {
            return Err(AppError::new(
                "BUNDLED_ADB_SERVER_UNVERIFIED",
                format!(
                    "端口 {} 上的 ADB 服务版本应为 {expected_version}，实际为 {}",
                    self.bundled_server_port, identity.version
                ),
                &recovery,
            ));
        }
        if !same_path(&identity.executable_absolute_path, adb) {
            return Err(AppError::new(
                "BUNDLED_ADB_SERVER_UNVERIFIED",
                format!(
                    "端口 {} 上的 ADB 服务不属于本应用",
                    self.bundled_server_port
                ),
                &recovery,
            ));
        }
        Ok(())
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

async fn query_adb_server_identity(
    port: u16,
    limit: Duration,
) -> Result<AdbServerIdentity, AppError> {
    timeout(limit, query_adb_server_identity_inner(port))
        .await
        .map_err(|_| {
            AppError::new(
                "BUNDLED_ADB_SERVER_UNVERIFIED",
                format!("读取端口 {port} 上的 ADB 服务状态超时"),
                "请关闭占用该端口的程序后重试，或选择一个外部 adb.exe",
            )
        })?
}

async fn query_adb_server_identity_inner(port: u16) -> Result<AdbServerIdentity, AppError> {
    use prost::Message;

    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|error| adb_server_protocol_error(port, error))?;
    let service = b"host:server-status";
    let request = format!("{:04x}", service.len());
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|error| adb_server_protocol_error(port, error))?;
    stream
        .write_all(service)
        .await
        .map_err(|error| adb_server_protocol_error(port, error))?;

    let mut status = [0_u8; 4];
    stream
        .read_exact(&mut status)
        .await
        .map_err(|error| adb_server_protocol_error(port, error))?;
    if &status != b"OKAY" && &status != b"FAIL" {
        return Err(AppError::new(
            "BUNDLED_ADB_SERVER_UNVERIFIED",
            format!("端口 {port} 返回了无效的 ADB 协议状态"),
            "请关闭占用该端口的程序后重试，或选择一个外部 adb.exe",
        ));
    }
    let payload_length = read_adb_protocol_length(&mut stream, port).await?;
    if payload_length > MAX_ADB_SERVER_STATUS_BYTES {
        return Err(AppError::new(
            "BUNDLED_ADB_SERVER_UNVERIFIED",
            format!("端口 {port} 返回的 ADB 服务状态数据过大"),
            "请关闭占用该端口的程序后重试，或选择一个外部 adb.exe",
        ));
    }
    let mut payload = vec![0_u8; payload_length];
    stream
        .read_exact(&mut payload)
        .await
        .map_err(|error| adb_server_protocol_error(port, error))?;
    if &status == b"FAIL" {
        return Err(AppError::new(
            "BUNDLED_ADB_SERVER_UNVERIFIED",
            format!(
                "端口 {port} 拒绝返回 ADB 服务状态：{}",
                String::from_utf8_lossy(&payload).trim()
            ),
            "请关闭占用该端口的程序后重试，或选择一个外部 adb.exe",
        ));
    }
    let server_status = AdbServerStatusProto::decode(payload.as_slice())
        .map_err(|error| adb_server_protocol_error(port, error))?;
    if server_status.version.is_empty() || server_status.executable_absolute_path.is_empty() {
        return Err(AppError::new(
            "BUNDLED_ADB_SERVER_UNVERIFIED",
            format!("端口 {port} 返回的 ADB 服务身份不完整"),
            "请关闭占用该端口的程序后重试，或选择一个外部 adb.exe",
        ));
    }
    Ok(AdbServerIdentity {
        version: server_status.version,
        executable_absolute_path: PathBuf::from(server_status.executable_absolute_path),
    })
}

async fn read_adb_protocol_length(stream: &mut TcpStream, port: u16) -> Result<usize, AppError> {
    let mut encoded = [0_u8; 4];
    stream
        .read_exact(&mut encoded)
        .await
        .map_err(|error| adb_server_protocol_error(port, error))?;
    let encoded =
        std::str::from_utf8(&encoded).map_err(|error| adb_server_protocol_error(port, error))?;
    usize::from_str_radix(encoded, 16).map_err(|error| adb_server_protocol_error(port, error))
}

fn adb_server_protocol_error(port: u16, error: impl std::fmt::Display) -> AppError {
    AppError::new(
        "BUNDLED_ADB_SERVER_UNVERIFIED",
        format!("无法读取端口 {port} 上的 ADB 服务状态：{error}"),
        "请关闭占用该端口的程序后重试，或选择一个外部 adb.exe",
    )
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
    recovery: &str,
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

fn ensure_bundled_server_port_available(port: u16) -> Result<(), AppError> {
    TcpListener::bind(("127.0.0.1", port))
        .map(drop)
        .map_err(|error| {
            AppError::new(
                "BUNDLED_ADB_PORT_UNAVAILABLE",
                format!("应用内置 ADB 的专用端口 {port} 不可用：{error}"),
                "请关闭占用该端口的程序后重试，或选择一个外部 adb.exe",
            )
        })
}

fn parse_bundled_adb_distribution() -> Result<BundledAdbDistribution, AppError> {
    serde_json::from_str(BUNDLED_ADB_DISTRIBUTION_JSON).map_err(|error| {
        AppError::new(
            "ADB_INVALID",
            format!("无法读取内置 ADB 的分发清单：{error}"),
            "请重新安装应用，或手动选择一个有效的外部 adb.exe",
        )
    })
}

fn validate_bundled_adb_files(
    adb_path: &Path,
    distribution: &BundledAdbDistribution,
) -> Result<(), AppError> {
    let directory = adb_path.parent().ok_or_else(|| {
        AppError::new(
            "ADB_INVALID",
            "内置 ADB 路径缺少安装目录",
            "请重新安装应用，或手动选择一个有效的外部 adb.exe",
        )
    })?;

    for file in &distribution.files {
        let path = directory.join(&file.name);
        let bytes = fs::read(&path).map_err(|error| {
            AppError::new(
                "ADB_INVALID",
                format!("无法读取内置 ADB 文件 {}：{error}", file.name),
                "请重新安装应用，或手动选择一个有效的外部 adb.exe",
            )
        })?;
        let actual_sha256 = format!("{:x}", Sha256::digest(bytes));
        if actual_sha256 != file.sha256 {
            return Err(AppError::new(
                "ADB_INVALID",
                format!("内置 ADB 文件 {} 校验失败", file.name),
                "请重新安装应用，或手动选择一个有效的外部 adb.exe",
            ));
        }
    }

    Ok(())
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
        .or_else(|| {
            candidates
                .iter()
                .find(|candidate| candidate.source == AdbSource::Bundled)
                .cloned()
        })
        .or_else(|| (candidates.len() == 1).then(|| candidates[0].clone()))
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn discover_adb_paths(
    saved: Option<&Path>,
    bundled_adb_path: Option<&Path>,
) -> Vec<(PathBuf, AdbSource)> {
    let mut candidates = Vec::new();
    if let Some(path) = bundled_adb_path {
        candidates.push((path.to_path_buf(), AdbSource::Bundled));
    }
    if let Some(path) = saved {
        candidates.push((path.to_path_buf(), AdbSource::Saved));
    }
    if let Some(path) = find_on_path() {
        candidates.push((path, AdbSource::Path));
    }
    candidates.extend(platform_adb_paths());

    let mut seen = HashSet::new();
    candidates.retain(|(path, _)| {
        seen.insert(path.canonicalize().unwrap_or_else(|_| path.clone()))
            && (path.is_file() || bundled_adb_path.is_some_and(|bundled| same_path(bundled, path)))
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
    use std::{
        collections::VecDeque,
        net::TcpListener,
        path::{Path, PathBuf},
        sync::Arc,
        time::Duration,
    };

    use async_trait::async_trait;
    use tokio::sync::Mutex;

    use super::{
        AdbCandidate, AdbServerIdentity, AdbServerProbe, AdbServerStatusProto, AdbSource, AppError,
        BundledAdbDistribution, BundledAdbFile, CommandOutput, CommandRunner, ConnectEndpoint,
        DeviceManager, Point, ensure_bundled_server_port_available, ensure_success, parse_devices,
        parse_endpoint, query_adb_server_identity, select_initial_adb, strings,
        validate_bundled_adb_files, validate_png,
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

    struct FakeServerProbe {
        identities: Mutex<VecDeque<Result<AdbServerIdentity, AppError>>>,
        ports: Mutex<Vec<u16>>,
    }

    impl FakeServerProbe {
        fn with_identities(identities: Vec<AdbServerIdentity>) -> Self {
            Self {
                identities: Mutex::new(identities.into_iter().map(Ok).collect()),
                ports: Mutex::new(Vec::new()),
            }
        }

        fn empty() -> Self {
            Self::with_identities(Vec::new())
        }

        async fn ports(&self) -> Vec<u16> {
            self.ports.lock().await.clone()
        }
    }

    #[async_trait]
    impl AdbServerProbe for FakeServerProbe {
        async fn identity(
            &self,
            port: u16,
            _limit: Duration,
        ) -> Result<AdbServerIdentity, AppError> {
            self.ports.lock().await.push(port);
            self.identities
                .lock()
                .await
                .pop_front()
                .expect("queued server identity")
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

    fn bundled_server_identity(adb_path: &Path) -> AdbServerIdentity {
        AdbServerIdentity {
            version: "37.0.1".to_owned(),
            executable_absolute_path: adb_path.to_owned(),
        }
    }

    #[tokio::test]
    async fn reads_server_identity_directly_from_the_adb_smart_socket() {
        use prost::Message;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let expected_path = PathBuf::from(r"C:\Program Files\OnmyojiSupportTools\adb\adb.exe");
        let payload = AdbServerStatusProto {
            version: "37.0.1".to_owned(),
            build: "14129643".to_owned(),
            executable_absolute_path: expected_path.to_string_lossy().into_owned(),
        }
        .encode_to_vec();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut length = [0_u8; 4];
            stream.read_exact(&mut length).await.unwrap();
            let length = usize::from_str_radix(std::str::from_utf8(&length).unwrap(), 16).unwrap();
            let mut request = vec![0_u8; length];
            stream.read_exact(&mut request).await.unwrap();
            assert_eq!(request, b"host:server-status");
            stream.write_all(b"OKAY").await.unwrap();
            stream
                .write_all(format!("{:04x}", payload.len()).as_bytes())
                .await
                .unwrap();
            stream.write_all(&payload).await.unwrap();
        });

        let identity = query_adb_server_identity(port, Duration::from_secs(1))
            .await
            .unwrap();

        assert_eq!(identity.version, "37.0.1");
        assert_eq!(identity.executable_absolute_path, expected_path);
        server.await.unwrap();
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
    fn selects_a_saved_then_bundled_then_only_adb_candidate() {
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
        let bundled = AdbCandidate {
            path: "bundled-adb.exe".to_owned(),
            source: AdbSource::Bundled,
            version: "37.0.1".to_owned(),
        };

        assert_eq!(
            select_initial_adb(std::slice::from_ref(&first), None)
                .unwrap()
                .path,
            first.path
        );
        assert!(select_initial_adb(&[first.clone(), second.clone()], None).is_none());
        assert_eq!(
            select_initial_adb(&[first.clone(), bundled.clone(), second.clone()], None)
                .unwrap()
                .path,
            bundled.path
        );
        assert_eq!(
            select_initial_adb(
                &[first, bundled, second.clone()],
                Some(Path::new("second-adb.exe")),
            )
            .unwrap()
            .path,
            second.path
        );
    }

    #[test]
    fn refuses_to_claim_a_bundled_adb_port_that_another_process_owns() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        let error = ensure_bundled_server_port_available(port).unwrap_err();

        assert_eq!(error.code, "BUNDLED_ADB_PORT_UNAVAILABLE");
    }

    #[test]
    fn rejects_a_tampered_bundled_adb_file_at_runtime() {
        let directory = tempfile::tempdir().unwrap();
        let adb_path = directory.path().join("adb.exe");
        std::fs::write(&adb_path, b"tampered").unwrap();
        let distribution = BundledAdbDistribution {
            version: "37.0.1".to_owned(),
            files: vec![BundledAdbFile {
                name: "adb.exe".to_owned(),
                sha256: "0000000000000000000000000000000000000000000000000000000000000000"
                    .to_owned(),
            }],
        };

        let error = validate_bundled_adb_files(&adb_path, &distribution).unwrap_err();

        assert_eq!(error.code, "ADB_INVALID");
        assert!(error.message.contains("校验失败"));
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
    async fn reports_a_missing_bundled_adb_as_a_recoverable_error() {
        let directory = tempfile::tempdir().unwrap();
        let runner = Arc::new(FakeRunner::with_outputs(Vec::new()));
        let manager = DeviceManager::with_runner_and_bundled_adb(
            directory.path().join("config.json"),
            directory.path().join("missing-adb.exe"),
            runner.clone(),
            Arc::new(FakeServerProbe::empty()),
        );

        let error = manager.initialize().await.unwrap_err();

        assert_eq!(error.code, "BUNDLED_ADB_INVALID");
        assert!(error.recovery.as_deref().unwrap().contains("外部 adb.exe"));
        assert!(runner.calls().await.is_empty());
    }

    #[tokio::test]
    async fn rejects_a_bundled_adb_with_an_unexpected_version() {
        let directory = tempfile::tempdir().unwrap();
        let adb_path = directory.path().join("adb.exe");
        std::fs::write(&adb_path, []).unwrap();
        let runner = Arc::new(FakeRunner::with_outputs(vec![
            success("Android Debug Bridge version 1.0.41\nVersion 35.0.2-12147458"),
            success(""),
            success("List of devices attached\n"),
        ]));
        let manager = DeviceManager::with_runner_and_bundled_adb(
            directory.path().join("config.json"),
            adb_path,
            runner,
            Arc::new(FakeServerProbe::empty()),
        );

        let error = manager.initialize().await.unwrap_err();

        assert_eq!(error.code, "BUNDLED_ADB_INVALID");
        assert!(error.message.contains("37.0.1"));
    }

    #[tokio::test]
    async fn initializes_with_the_bundled_adb_when_no_selection_is_saved() {
        let directory = tempfile::tempdir().unwrap();
        let adb_path = directory.path().join("adb.exe");
        std::fs::write(&adb_path, []).unwrap();
        let runner = Arc::new(FakeRunner::with_outputs(vec![
            success("Android Debug Bridge version 1.0.41\nVersion 37.0.1-14129643"),
            success(""),
            success("List of devices attached\n"),
            success(""),
        ]));
        let server_probe = Arc::new(FakeServerProbe::with_identities(vec![
            bundled_server_identity(&adb_path),
            bundled_server_identity(&adb_path),
        ]));
        let manager = DeviceManager::with_runner_and_bundled_adb(
            directory.path().join("config.json"),
            adb_path.clone(),
            runner.clone(),
            server_probe.clone(),
        );

        let state = manager.initialize().await.unwrap();
        let selected = state.selected_adb.unwrap();

        assert_eq!(selected.path, adb_path.to_string_lossy());
        assert_eq!(selected.source, AdbSource::Bundled);
        assert_eq!(selected.version, "37.0.1-14129643");
        assert_eq!(runner.calls().await[0], strings(&["-P", "5038", "version"]));
        assert_eq!(
            runner.calls().await[1],
            strings(&["-P", "5038", "start-server"])
        );
        assert_eq!(
            runner.calls().await[2],
            strings(&["-P", "5038", "devices", "-l"])
        );
        manager.shutdown().await.unwrap();
        assert_eq!(
            runner.calls().await[3],
            strings(&["-P", "5038", "kill-server"])
        );
        assert_eq!(server_probe.ports().await, vec![5038, 5038]);
    }

    #[tokio::test]
    async fn reclaims_a_stale_server_started_from_the_same_bundled_adb() {
        let directory = tempfile::tempdir().unwrap();
        let adb_path = directory.path().join("adb.exe");
        std::fs::write(&adb_path, []).unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let port_text = port.to_string();
        let runner = Arc::new(FakeRunner::with_outputs(vec![
            success("Android Debug Bridge version 1.0.41\nVersion 37.0.1-14129643"),
            success("List of devices attached\n"),
            success(""),
        ]));
        let server_probe = Arc::new(FakeServerProbe::with_identities(vec![
            bundled_server_identity(&adb_path),
            bundled_server_identity(&adb_path),
        ]));
        let manager = DeviceManager::with_runner_and_bundled_adb_on_port(
            directory.path().join("config.json"),
            adb_path,
            port,
            runner.clone(),
            server_probe.clone(),
        );

        manager.initialize().await.unwrap();
        manager.shutdown().await.unwrap();

        let calls = runner.calls().await;
        assert_eq!(calls[1], strings(&["-P", &port_text, "devices", "-l"]));
        assert!(
            !calls
                .iter()
                .any(|call| call.last().unwrap() == "start-server")
        );
        assert_eq!(calls[2], strings(&["-P", &port_text, "kill-server"]));
        assert_eq!(server_probe.ports().await, vec![port, port]);
    }

    #[tokio::test]
    async fn refuses_to_adopt_or_stop_a_server_started_from_another_adb() {
        let directory = tempfile::tempdir().unwrap();
        let adb_path = directory.path().join("adb.exe");
        let other_adb_path = directory.path().join("other-adb.exe");
        std::fs::write(&adb_path, []).unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let runner = Arc::new(FakeRunner::with_outputs(vec![success(
            "Android Debug Bridge version 1.0.41\nVersion 37.0.1-14129643",
        )]));
        let server_probe = Arc::new(FakeServerProbe::with_identities(vec![
            bundled_server_identity(&other_adb_path),
        ]));
        let manager = DeviceManager::with_runner_and_bundled_adb_on_port(
            directory.path().join("config.json"),
            adb_path,
            port,
            runner.clone(),
            server_probe,
        );

        let error = manager.initialize().await.unwrap_err();
        manager.shutdown().await.unwrap();

        assert_eq!(error.code, "BUNDLED_ADB_PORT_UNAVAILABLE");
        let calls = runner.calls().await;
        assert!(
            !calls
                .iter()
                .any(|call| call.last().unwrap() == "start-server")
        );
        assert!(
            !calls
                .iter()
                .any(|call| call.last().unwrap() == "kill-server")
        );
    }

    #[tokio::test]
    async fn refuses_to_adopt_an_old_server_from_the_bundled_path() {
        let directory = tempfile::tempdir().unwrap();
        let adb_path = directory.path().join("adb.exe");
        std::fs::write(&adb_path, []).unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let runner = Arc::new(FakeRunner::with_outputs(vec![success(
            "Android Debug Bridge version 1.0.41\nVersion 37.0.1-14129643",
        )]));
        let server_probe = Arc::new(FakeServerProbe::with_identities(vec![AdbServerIdentity {
            version: "36.0.0".to_owned(),
            executable_absolute_path: adb_path.clone(),
        }]));
        let manager = DeviceManager::with_runner_and_bundled_adb_on_port(
            directory.path().join("config.json"),
            adb_path,
            port,
            runner.clone(),
            server_probe,
        );

        let error = manager.initialize().await.unwrap_err();
        manager.shutdown().await.unwrap();

        assert_eq!(error.code, "BUNDLED_ADB_PORT_UNAVAILABLE");
        assert_eq!(runner.calls().await.len(), 1);
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
