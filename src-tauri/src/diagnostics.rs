use std::{
    fs::{self, File, OpenOptions},
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use rand::Rng;
use serde::Serialize;

use crate::device::AppError;

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const FILE_COUNT: usize = 3;
const MAX_SUMMARY_CHARS: usize = 160;
static DIAGNOSTICS: OnceLock<Diagnostics> = OnceLock::new();

tokio::task_local! {
    static OPERATION: OperationContext;
}

struct OperationContext {
    trace_id: String,
    name: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Entry<'a> {
    timestamp_ms: u128,
    session_id: &'a str,
    trace_id: &'a str,
    operation: &'a str,
    step: &'a str,
    outcome: &'a str,
    code: Option<&'a str>,
    recovery_action: Option<&'a str>,
    elapsed_ms: Option<u128>,
    command: Option<&'a str>,
    timeout_ms: Option<u128>,
    exit_code: Option<i32>,
    stdout_summary: Option<&'a str>,
    stderr_summary: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<Metadata<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Metadata<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    platform: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    architecture: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    adb_source: Option<&'a str>,
}

struct Writer {
    file: Option<File>,
}

pub struct Diagnostics {
    dir: PathBuf,
    session_id: String,
    writer: Mutex<Writer>,
}

fn random_id() -> String {
    format!("{:032x}", rand::rng().random::<u128>())
}

fn timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

impl Diagnostics {
    fn new(dir: PathBuf) -> Self {
        let file = fs::create_dir_all(&dir)
            .and_then(|_| {
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join("diagnostics.jsonl"))
            })
            .ok();
        Self {
            dir,
            session_id: random_id(),
            writer: Mutex::new(Writer { file }),
        }
    }

    pub fn available(&self) -> bool {
        self.writer.lock().is_ok_and(|writer| writer.file.is_some())
    }

    fn file_path(&self, index: usize) -> PathBuf {
        if index == 0 {
            self.dir.join("diagnostics.jsonl")
        } else {
            self.dir.join(format!("diagnostics.{index}.jsonl"))
        }
    }

    fn rotate(&self, writer: &mut Writer) -> std::io::Result<()> {
        writer.file.take();
        let _ = fs::remove_file(self.file_path(FILE_COUNT - 1));
        for index in (0..FILE_COUNT - 1).rev() {
            let from = self.file_path(index);
            if from.exists() {
                fs::rename(from, self.file_path(index + 1))?;
            }
        }
        writer.file = Some(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.file_path(0))?,
        );
        Ok(())
    }

    fn write(&self, entry: &Entry<'_>) {
        let Ok(mut writer) = self.writer.lock() else {
            return;
        };
        if writer.file.is_none() {
            return;
        }
        let Ok(mut line) = serde_json::to_vec(entry) else {
            return;
        };
        line.push(b'\n');
        let result = (|| {
            let length = writer
                .file
                .as_ref()
                .expect("checked above")
                .metadata()?
                .len();
            if length + line.len() as u64 > MAX_FILE_BYTES {
                self.rotate(&mut writer)?;
            }
            writer
                .file
                .as_mut()
                .expect("rotation opened file")
                .write_all(&line)
        })();
        if result.is_err() {
            writer.file = None;
        }
    }

    pub fn export(&self, destination: &Path) -> Result<(), AppError> {
        let Ok(mut writer) = self.writer.lock() else {
            return Err(unavailable());
        };
        let Some(file) = writer.file.as_mut() else {
            return Err(unavailable());
        };
        if (0..FILE_COUNT).any(|index| {
            let existing = self.file_path(index);
            existing == destination
                || (existing.exists()
                    && destination.canonicalize().ok() == existing.canonicalize().ok())
        }) {
            return Err(AppError::new(
                "DIAGNOSTICS_EXPORT_FAILED",
                "不能覆盖正在使用的诊断日志",
                "请选择日志目录以外的保存位置",
            ));
        }
        file.flush().map_err(|_| unavailable())?;
        let mut output = Vec::new();
        for index in (0..FILE_COUNT).rev() {
            let path = self.file_path(index);
            if path.exists() {
                output.extend(fs::read(path).map_err(|_| unavailable())?);
            }
        }
        fs::write(destination, output).map_err(|_| {
            AppError::new(
                "DIAGNOSTICS_EXPORT_FAILED",
                "无法保存诊断日志",
                "请选择可写入的位置后重试",
            )
        })
    }

    fn write_command_result(
        &self,
        trace_id: &str,
        operation: &str,
        command: &'static str,
        limit: std::time::Duration,
        elapsed: std::time::Duration,
        result: Result<CommandResult<'_>, &'static str>,
    ) {
        let (outcome, code, exit_code, stdout, stderr) = match result {
            Ok(result) => (
                if result.success { "success" } else { "failure" },
                if result.success {
                    None
                } else {
                    Some("ADB_NONZERO_EXIT")
                },
                result.exit_code,
                Some(if command == "screencap" || command == "screenrecord" {
                    format!("binary ({} bytes)", result.stdout.len())
                } else {
                    safe_summary(result.stdout)
                }),
                Some(safe_summary(result.stderr)),
            ),
            Err(code) => ("failure", Some(code), None, None, None),
        };
        self.write(&Entry {
            timestamp_ms: timestamp_ms(),
            session_id: &self.session_id,
            trace_id,
            operation,
            step: "adb_command",
            outcome,
            code,
            recovery_action: code.map(recovery_action),
            elapsed_ms: Some(elapsed.as_millis()),
            command: Some(command),
            timeout_ms: Some(limit.as_millis()),
            exit_code,
            stdout_summary: stdout.as_deref(),
            stderr_summary: stderr.as_deref(),
            metadata: None,
        });
    }

    pub fn directory(&self) -> Result<&Path, AppError> {
        self.available()
            .then_some(self.dir.as_path())
            .ok_or_else(unavailable)
    }
}

fn unavailable() -> AppError {
    AppError::new(
        "DIAGNOSTICS_UNAVAILABLE",
        "诊断日志不可用",
        "请检查应用日志目录权限后重启应用",
    )
}

pub fn initialize(dir: PathBuf) {
    let diagnostics = DIAGNOSTICS.get_or_init(|| Diagnostics::new(dir));
    diagnostics.write(&Entry {
        timestamp_ms: timestamp_ms(),
        session_id: &diagnostics.session_id,
        trace_id: &diagnostics.session_id,
        operation: "app",
        step: "startup",
        outcome: "success",
        code: None,
        recovery_action: None,
        elapsed_ms: None,
        command: None,
        timeout_ms: None,
        exit_code: None,
        stdout_summary: None,
        stderr_summary: None,
        metadata: Some(Metadata {
            version: Some(env!("CARGO_PKG_VERSION")),
            platform: Some(std::env::consts::OS),
            architecture: Some(std::env::consts::ARCH),
            adb_source: None,
        }),
    });
}

pub fn status() -> bool {
    DIAGNOSTICS.get().is_some_and(Diagnostics::available)
}

pub fn directory() -> Result<&'static Path, AppError> {
    DIAGNOSTICS.get().ok_or_else(unavailable)?.directory()
}

pub fn export(destination: &Path) -> Result<(), AppError> {
    DIAGNOSTICS
        .get()
        .ok_or_else(unavailable)?
        .export(destination)
}

fn valid_trace_id(value: &str) -> bool {
    (8..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

pub async fn operation<T, F>(
    name: &'static str,
    trace_id: Option<String>,
    future: F,
) -> Result<T, AppError>
where
    F: Future<Output = Result<T, AppError>>,
{
    let trace_id = trace_id
        .filter(|value| valid_trace_id(value))
        .unwrap_or_else(random_id);
    OPERATION
        .scope(OperationContext { trace_id, name }, async {
            let start = Instant::now();
            record("ipc", "start", None, None);
            let result = future.await;
            record(
                "ipc",
                if result.is_ok() { "success" } else { "failure" },
                result.as_ref().err().map(|error| error.code),
                Some(start.elapsed()),
            );
            result
        })
        .await
}

pub async fn step<T, F>(name: &'static str, future: F) -> Result<T, AppError>
where
    F: Future<Output = Result<T, AppError>>,
{
    let start = Instant::now();
    record(name, "start", None, None);
    let result = future.await;
    record(
        name,
        if result.is_ok() { "success" } else { "failure" },
        result.as_ref().err().map(|error| error.code),
        Some(start.elapsed()),
    );
    result
}

pub fn sync_step<T, F>(name: &'static str, work: F) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError>,
{
    let start = Instant::now();
    record(name, "start", None, None);
    let result = work();
    record(
        name,
        if result.is_ok() { "success" } else { "failure" },
        result.as_ref().err().map(|error| error.code),
        Some(start.elapsed()),
    );
    result
}

fn recovery_action(code: &str) -> &'static str {
    match code {
        "ADB_NOT_FOUND" | "ADB_INVALID" | "BUNDLED_ADB_INVALID" => "select_valid_adb",
        "BUNDLED_ADB_SERVER_UNVERIFIED" | "BUNDLED_ADB_START_FAILED" => {
            "select_external_adb_or_retry"
        }
        "DEVICE_NOT_FOUND" | "DEVICE_NOT_READY" | "DEVICE_OFFLINE" | "DEVICE_UNAUTHORIZED" => {
            "refresh_and_select_online_device"
        }
        "FRAME_REQUIRED" | "DEVICE_CHANGED" | "CLICK_CANCELLED" => "capture_current_frame_again",
        "PREVIEW_STREAM_EOF"
        | "PREVIEW_DECODE_FAILED"
        | "PREVIEW_START_FAILED"
        | "PREVIEW_READ_FAILED"
        | "PREVIEW_STREAM_STALLED"
        | "PREVIEW_PROCESS_EXITED" => "use_static_capture",
        "ADB_TIMEOUT" | "ADB_NONZERO_EXIT" | "CAPTURE_FAILED" | "TAP_FAILED" => {
            "check_device_and_retry"
        }
        _ => "see_app_recovery_message",
    }
}

pub fn record(
    step: &'static str,
    outcome: &'static str,
    code: Option<&'static str>,
    elapsed: Option<std::time::Duration>,
) {
    let Some(diagnostics) = DIAGNOSTICS.get() else {
        return;
    };
    let context = OPERATION
        .try_with(|context| (context.trace_id.clone(), context.name))
        .ok();
    let (trace_id, operation) = context
        .as_ref()
        .map(|(id, name)| (id.as_str(), *name))
        .unwrap_or((diagnostics.session_id.as_str(), "app"));
    let outcome = match outcome {
        "start" | "success" | "failure" | "stop" | "ended" | "info" => outcome,
        _ => "info",
    };
    diagnostics.write(&Entry {
        timestamp_ms: timestamp_ms(),
        session_id: &diagnostics.session_id,
        trace_id,
        operation,
        step,
        outcome,
        code,
        recovery_action: code.map(recovery_action),
        elapsed_ms: elapsed.map(|value| value.as_millis()),
        command: None,
        timeout_ms: None,
        exit_code: None,
        stdout_summary: None,
        stderr_summary: None,
        metadata: None,
    });
}

pub fn adb_source(source: &'static str) {
    let Some(diagnostics) = DIAGNOSTICS.get() else {
        return;
    };
    let context = OPERATION
        .try_with(|context| (context.trace_id.clone(), context.name))
        .ok();
    let (trace_id, operation) = context
        .as_ref()
        .map(|(id, name)| (id.as_str(), *name))
        .unwrap_or((diagnostics.session_id.as_str(), "app"));
    diagnostics.write(&Entry {
        timestamp_ms: timestamp_ms(),
        session_id: &diagnostics.session_id,
        trace_id,
        operation,
        step: "adb_source",
        outcome: "info",
        code: None,
        recovery_action: None,
        elapsed_ms: None,
        command: None,
        timeout_ms: None,
        exit_code: None,
        stdout_summary: None,
        stderr_summary: None,
        metadata: Some(Metadata {
            version: None,
            platform: None,
            architecture: None,
            adb_source: Some(source),
        }),
    });
}

pub fn command_class(args: &[String]) -> &'static str {
    for (needle, class) in [
        ("version", "version"),
        ("devices", "devices"),
        ("connect", "connect"),
        ("disconnect", "disconnect"),
        ("screencap", "screencap"),
        ("screenrecord", "screenrecord"),
        ("swipe", "tap"),
        ("start-server", "start-server"),
        ("kill-server", "kill-server"),
        ("getprop", "device-model"),
    ] {
        if args.iter().any(|argument| argument == needle) {
            return class;
        }
    }
    "other"
}

pub fn current_trace_id() -> Option<String> {
    OPERATION.try_with(|context| context.trace_id.clone()).ok()
}

pub fn preview_end(
    trace_id: Option<String>,
    code: &'static str,
    exit_code: Option<i32>,
    stderr: &[u8],
    elapsed: std::time::Duration,
) {
    let Some(diagnostics) = DIAGNOSTICS.get() else {
        return;
    };
    let trace_id = trace_id.unwrap_or_else(|| diagnostics.session_id.clone());
    let summary = safe_summary(stderr);
    diagnostics.write(&Entry {
        timestamp_ms: timestamp_ms(),
        session_id: &diagnostics.session_id,
        trace_id: &trace_id,
        operation: "start_preview",
        step: "preview_end",
        outcome: if code == "PREVIEW_STREAM_EOF" {
            "ended"
        } else {
            "failure"
        },
        code: Some(code),
        recovery_action: Some(recovery_action(code)),
        elapsed_ms: Some(elapsed.as_millis()),
        command: Some("screenrecord"),
        timeout_ms: None,
        exit_code,
        stdout_summary: None,
        stderr_summary: Some(&summary),
        metadata: None,
    });
}

pub fn safe_summary(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "empty".to_owned();
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return format!("binary ({} bytes)", bytes.len());
    };
    let mut summary = String::new();
    for token in text.split_whitespace() {
        let safe = match token.to_ascii_lowercase().as_str() {
            "error" | "failed" | "failure" | "device" | "devices" | "offline" | "unauthorized"
            | "connected" | "disconnected" | "cannot" | "timeout" | "daemon" | "server"
            | "started" | "stopped" | "already" | "running" => token,
            _ => "[redacted]",
        };
        if summary.len() + safe.len() + 1 > MAX_SUMMARY_CHARS {
            break;
        }
        if !summary.is_empty() {
            summary.push(' ')
        }
        summary.push_str(safe);
    }
    if summary.is_empty() {
        "[redacted]".to_owned()
    } else {
        summary
    }
}

pub struct CommandResult<'a> {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: &'a [u8],
    pub stderr: &'a [u8],
}

pub fn command_result(
    command: &'static str,
    limit: std::time::Duration,
    elapsed: std::time::Duration,
    result: Result<CommandResult<'_>, &'static str>,
) {
    let Some(diagnostics) = DIAGNOSTICS.get() else {
        return;
    };
    let context = OPERATION
        .try_with(|context| (context.trace_id.clone(), context.name))
        .ok();
    let (trace_id, operation) = context
        .as_ref()
        .map(|(id, name)| (id.as_str(), *name))
        .unwrap_or((diagnostics.session_id.as_str(), "app"));
    diagnostics.write_command_result(trace_id, operation, command, limit, elapsed, result);
}

pub fn frontend_event(trace_id: String, code: &str) {
    let code = match code {
        "IPC_FAILED" => "IPC_FAILED",
        "FRONTEND_UNHANDLED" => "FRONTEND_UNHANDLED",
        "FRONTEND_REJECTION" => "FRONTEND_REJECTION",
        "PREVIEW_DECODE_FAILED" => "PREVIEW_DECODE_FAILED",
        _ => "FRONTEND_ERROR",
    };
    let Some(diagnostics) = DIAGNOSTICS.get() else {
        return;
    };
    let trace_id = if valid_trace_id(&trace_id) {
        trace_id
    } else {
        random_id()
    };
    diagnostics.write(&Entry {
        timestamp_ms: timestamp_ms(),
        session_id: &diagnostics.session_id,
        trace_id: &trace_id,
        operation: "frontend",
        step: "frontend_error",
        outcome: "failure",
        code: Some(code),
        recovery_action: Some(recovery_action(code)),
        elapsed_ms: None,
        command: None,
        timeout_ms: None,
        exit_code: None,
        stdout_summary: None,
        stderr_summary: None,
        metadata: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_redact_before_truncating_and_exclude_binary() {
        let input = format!(
            "error ABC123 192.168.1.9 C:\\Users\\private\\adb.exe {}",
            "secret".repeat(500)
        );
        let summary = safe_summary(input.as_bytes());
        assert!(summary.len() <= MAX_SUMMARY_CHARS);
        for private in ["ABC123", "192.168.1.9", "private", "adb.exe", "secret"] {
            assert!(!summary.contains(private));
        }
        assert_eq!(safe_summary(&[0x89, 0x50, 0x4e, 0x47]), "binary (4 bytes)");
    }

    #[test]
    fn rotation_export_and_unwritable_directory() {
        let temp = tempfile::tempdir().unwrap();
        let logger = Diagnostics::new(temp.path().join("logs"));
        let entry = Entry {
            timestamp_ms: 1,
            session_id: "session",
            trace_id: "trace",
            operation: "test",
            step: "step",
            outcome: "success",
            code: None,
            recovery_action: None,
            elapsed_ms: None,
            command: None,
            timeout_ms: None,
            exit_code: None,
            stdout_summary: None,
            stderr_summary: None,
            metadata: None,
        };
        for _ in 0..30000 {
            logger.write(&entry)
        }
        let mut total = 0;
        for index in 0..FILE_COUNT {
            total += fs::metadata(logger.file_path(index))
                .map(|data| data.len())
                .unwrap_or(0);
        }
        assert!(total <= MAX_FILE_BYTES * FILE_COUNT as u64);
        for index in 0..FILE_COUNT {
            assert!(
                fs::metadata(logger.file_path(index))
                    .map(|data| data.len())
                    .unwrap_or(0)
                    <= MAX_FILE_BYTES
            );
        }
        let destination = temp.path().join("export.jsonl");
        logger.export(&destination).unwrap();
        assert!(!fs::read(destination).unwrap().is_empty());
        assert!(logger.export(&logger.file_path(0)).is_err());

        let blocked = temp.path().join("file");
        fs::write(&blocked, "x").unwrap();
        let unavailable = Diagnostics::new(blocked.join("logs"));
        assert!(!unavailable.available());
        unavailable.write(&entry);
        assert!(unavailable.export(&temp.path().join("no-export")).is_err());
    }

    #[tokio::test]
    async fn operation_carries_trace_id_across_await() {
        let result = operation("refresh_devices", Some("aabbccdd-1122".into()), async {
            tokio::task::yield_now().await;
            Ok(OPERATION.with(|current| (current.trace_id.clone(), current.name)))
        })
        .await;
        assert_eq!(
            result.unwrap(),
            ("aabbccdd-1122".to_owned(), "refresh_devices")
        );
    }

    #[tokio::test]
    async fn step_results_keep_trace_duration_code_and_recovery_action() {
        let temp = tempfile::tempdir().unwrap();
        initialize(temp.path().join("logs"));
        let trace_id = "00112233-aabb";
        let result = operation("capture_screen", Some(trace_id.into()), async {
            step("active_device", async { Ok(()) }).await?;
            let failure = sync_step("png_validation", || {
                Err::<(), _>(AppError::new("CAPTURE_FAILED", "截图失败", "请重试"))
            });
            assert!(failure.is_err());
            Ok(())
        })
        .await;
        assert!(result.is_ok());

        let lines = fs::read_to_string(temp.path().join("logs/diagnostics.jsonl")).unwrap();
        let entries: Vec<serde_json::Value> = lines
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .filter(|entry: &serde_json::Value| entry["traceId"] == trace_id)
            .collect();
        let steps: Vec<_> = entries
            .iter()
            .map(|entry| {
                (
                    entry["step"].as_str().unwrap(),
                    entry["outcome"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            steps,
            [
                ("ipc", "start"),
                ("active_device", "start"),
                ("active_device", "success"),
                ("png_validation", "start"),
                ("png_validation", "failure"),
                ("ipc", "success"),
            ]
        );
        let success = &entries[2];
        assert!(success["elapsedMs"].is_number());
        let failure = &entries[4];
        assert!(failure["elapsedMs"].is_number());
        assert_eq!(failure["code"], "CAPTURE_FAILED");
        assert_eq!(failure["recoveryAction"], "check_device_and_retry");
    }

    #[test]
    fn command_results_keep_codes_timings_and_exit_status_without_private_values() {
        let temp = tempfile::tempdir().unwrap();
        let logger = Diagnostics::new(temp.path().join("logs"));
        logger.write_command_result(
            "aabbccdd",
            "refresh_devices",
            "devices",
            std::time::Duration::from_secs(8),
            std::time::Duration::from_millis(23),
            Ok(CommandResult {
                success: false,
                exit_code: Some(7),
                stdout: b"ABC123 192.168.1.9",
                stderr: b"error C:\\Users\\private\\adb.exe",
            }),
        );
        logger.write_command_result(
            "aabbccdd",
            "refresh_devices",
            "devices",
            std::time::Duration::from_secs(8),
            std::time::Duration::from_secs(8),
            Err("ADB_TIMEOUT"),
        );
        let lines = fs::read_to_string(logger.file_path(0)).unwrap();
        let entries: Vec<serde_json::Value> = lines
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(entries[0]["traceId"], "aabbccdd");
        assert_eq!(entries[0]["code"], "ADB_NONZERO_EXIT");
        assert_eq!(entries[0]["recoveryAction"], "check_device_and_retry");
        assert_eq!(entries[0]["exitCode"], 7);
        assert_eq!(entries[0]["elapsedMs"], 23);
        assert_eq!(entries[1]["code"], "ADB_TIMEOUT");
        assert_eq!(entries[1]["recoveryAction"], "check_device_and_retry");
        assert_eq!(
            recovery_action("PREVIEW_STREAM_STALLED"),
            "use_static_capture"
        );
        assert_eq!(recovery_action("PREVIEW_READ_FAILED"), "use_static_capture");
        assert_eq!(
            recovery_action("PREVIEW_PROCESS_EXITED"),
            "use_static_capture"
        );
        assert_eq!(entries[1]["timeoutMs"], 8000);
        for secret in ["ABC123", "192.168.1.9", "private", "adb.exe"] {
            assert!(!lines.contains(secret));
        }
    }
}
