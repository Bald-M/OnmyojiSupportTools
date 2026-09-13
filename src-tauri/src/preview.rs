use std::{
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use serde::Serialize;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    sync::Mutex,
    task::JoinHandle,
    time::timeout,
};

use crate::device::AppError;

pub(crate) type PreviewSink = Arc<dyn Fn(Vec<u8>) + Send + Sync>;
pub(crate) type PreviewEndSink = Arc<dyn Fn(PreviewEnd) + Send + Sync>;
const PREVIEW_STALL_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_PREVIEW_STDERR_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewEnd {
    pub code: PreviewEndCode,
    pub exit_code: Option<i32>,
    pub first_chunk_at: Option<u64>,
    pub last_chunk_at: Option<u64>,
    pub stderr: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PreviewEndCode {
    #[serde(rename = "PREVIEW_STREAM_EOF")]
    StreamEof,
    #[serde(rename = "PREVIEW_READ_FAILED")]
    ReadFailed,
    #[serde(rename = "PREVIEW_STREAM_STALLED")]
    StreamStalled,
    #[serde(rename = "PREVIEW_PROCESS_EXITED")]
    ProcessExited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamEnd {
    Eof,
    ReadFailed,
    Stalled,
}

struct StreamResult {
    end: StreamEnd,
    first_chunk_at: Option<u64>,
    last_chunk_at: Option<u64>,
}

fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

async fn forward_stream<R: AsyncRead + Unpin>(
    stdout: &mut R,
    stall_timeout: Duration,
    sink: &PreviewSink,
) -> StreamResult {
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut first_chunk_at = None;
    let mut last_chunk_at = None;
    let end = loop {
        match timeout(stall_timeout, stdout.read(&mut buffer)).await {
            Ok(Ok(0)) => break StreamEnd::Eof,
            Ok(Err(_)) => break StreamEnd::ReadFailed,
            Err(_) => break StreamEnd::Stalled,
            Ok(Ok(length)) => {
                let received_at = epoch_millis();
                first_chunk_at.get_or_insert(received_at);
                last_chunk_at = Some(received_at);
                sink(buffer[..length].to_vec());
            }
        }
    };
    StreamResult {
        end,
        first_chunk_at,
        last_chunk_at,
    }
}

async fn read_truncated_stderr(mut stderr: impl AsyncRead + Unpin) -> Vec<u8> {
    let mut retained = Vec::new();
    let mut buffer = [0_u8; 1024];
    while let Ok(length) = stderr.read(&mut buffer).await {
        if length == 0 {
            break;
        }
        let remaining = MAX_PREVIEW_STDERR_BYTES.saturating_sub(retained.len());
        retained.extend_from_slice(&buffer[..length.min(remaining)]);
    }
    retained
}

#[async_trait]
pub(crate) trait PreviewSessionHandle: Send {
    async fn stop(self: Box<Self>);
}

#[async_trait]
pub(crate) trait PreviewBackend: Send + Sync {
    async fn start(
        &self,
        program: &Path,
        args: &[String],
        sink: PreviewSink,
        on_end: PreviewEndSink,
        live: Arc<AtomicBool>,
    ) -> Result<Box<dyn PreviewSessionHandle>, AppError>;
}

pub(crate) struct AdbScreenrecordPreviewBackend;

struct ProcessPreviewSession {
    task: JoinHandle<()>,
}

#[async_trait]
impl PreviewSessionHandle for ProcessPreviewSession {
    async fn stop(self: Box<Self>) {
        self.task.abort();
        let _ = self.task.await;
    }
}

#[async_trait]
impl PreviewBackend for AdbScreenrecordPreviewBackend {
    async fn start(
        &self,
        program: &Path,
        args: &[String],
        sink: PreviewSink,
        on_end: PreviewEndSink,
        live: Arc<AtomicBool>,
    ) -> Result<Box<dyn PreviewSessionHandle>, AppError> {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| {
            AppError::new(
                "PREVIEW_START_FAILED",
                format!("无法启动实时预览原型：{error}"),
                "请确认设备在线且 Android 支持 screenrecord",
            )
        })?;
        let mut stdout = child.stdout.take().ok_or_else(|| {
            AppError::new(
                "PREVIEW_START_FAILED",
                "无法读取实时预览视频流",
                "请停止预览后重试",
            )
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            AppError::new(
                "PREVIEW_START_FAILED",
                "无法读取实时预览诊断信息",
                "请停止预览后重试",
            )
        })?;

        let task = tokio::spawn(async move {
            let stderr_task = tokio::spawn(read_truncated_stderr(stderr));
            let stream = forward_stream(&mut stdout, PREVIEW_STALL_TIMEOUT, &sink).await;
            if stream.end != StreamEnd::Eof {
                let _ = child.kill().await;
            }
            let status = child.wait().await.ok();
            let stderr = stderr_task.await.unwrap_or_default();
            let code = match (stream.end, status.as_ref()) {
                (StreamEnd::Stalled, _) => PreviewEndCode::StreamStalled,
                (StreamEnd::ReadFailed, _) => PreviewEndCode::ReadFailed,
                (StreamEnd::Eof, Some(status)) if !status.success() => {
                    PreviewEndCode::ProcessExited
                }
                (StreamEnd::Eof, _) => PreviewEndCode::StreamEof,
            };
            live.store(false, Ordering::Release);
            on_end(PreviewEnd {
                code,
                exit_code: status.and_then(|status| status.code()),
                first_chunk_at: stream.first_chunk_at,
                last_chunk_at: stream.last_chunk_at,
                stderr: (!stderr.is_empty())
                    .then(|| String::from_utf8_lossy(&stderr).trim().to_owned()),
            });
        });

        Ok(Box::new(ProcessPreviewSession { task }))
    }
}

struct ActivePreview {
    device_serial: String,
    session: Box<dyn PreviewSessionHandle>,
    live: Arc<AtomicBool>,
}

pub(crate) struct PreviewController {
    backend: Arc<dyn PreviewBackend>,
    active: Mutex<Option<ActivePreview>>,
}

impl PreviewController {
    pub(crate) fn new(backend: Arc<dyn PreviewBackend>) -> Self {
        Self {
            backend,
            active: Mutex::new(None),
        }
    }

    pub(crate) async fn start(
        &self,
        program: &Path,
        args: &[String],
        device_serial: String,
        sink: PreviewSink,
        on_end: PreviewEndSink,
    ) -> Result<(), AppError> {
        self.stop().await;
        let live = Arc::new(AtomicBool::new(true));
        let session = self
            .backend
            .start(program, args, sink, on_end, live.clone())
            .await?;
        *self.active.lock().await = Some(ActivePreview {
            device_serial,
            session,
            live,
        });
        Ok(())
    }

    pub(crate) async fn stop(&self) {
        if let Some(active) = self.active.lock().await.take() {
            active.session.stop().await;
        }
    }

    pub(crate) async fn active_device_serial(&self) -> Option<String> {
        let mut active = self.active.lock().await;
        if active
            .as_ref()
            .is_some_and(|preview| !preview.live.load(Ordering::Acquire))
        {
            active.take();
        }
        active.as_ref().map(|preview| preview.device_serial.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path, process::Command as StdCommand, sync::atomic::AtomicBool, time::Instant,
    };

    use super::*;
    use tokio::io::AsyncWriteExt;

    struct EndedBackend;
    struct EndedSession;

    #[async_trait]
    impl PreviewSessionHandle for EndedSession {
        async fn stop(self: Box<Self>) {}
    }

    #[async_trait]
    impl PreviewBackend for EndedBackend {
        async fn start(
            &self,
            _program: &Path,
            _args: &[String],
            _sink: PreviewSink,
            on_end: PreviewEndSink,
            live: Arc<AtomicBool>,
        ) -> Result<Box<dyn PreviewSessionHandle>, AppError> {
            live.store(false, Ordering::Release);
            on_end(PreviewEnd {
                code: PreviewEndCode::StreamEof,
                exit_code: Some(0),
                first_chunk_at: None,
                last_chunk_at: None,
                stderr: None,
            });
            Ok(Box::new(EndedSession))
        }
    }

    #[tokio::test]
    async fn completed_stream_is_not_reported_as_an_active_preview() {
        let controller = PreviewController::new(Arc::new(EndedBackend));
        let ended = Arc::new(AtomicBool::new(false));
        let ended_for_callback = ended.clone();

        controller
            .start(
                Path::new("adb.exe"),
                &[],
                "device-1".to_owned(),
                Arc::new(|_| {}),
                Arc::new(move |_| ended_for_callback.store(true, Ordering::Release)),
            )
            .await
            .unwrap();

        assert!(ended.load(Ordering::Acquire));
        assert!(controller.active_device_serial().await.is_none());
    }

    #[tokio::test]
    async fn watchdog_is_refreshed_by_each_stream_chunk() {
        let (mut writer, mut reader) = tokio::io::duplex(64);
        let received = Arc::new(std::sync::Mutex::new(Vec::new()));
        let received_for_sink = received.clone();
        let sink: PreviewSink =
            Arc::new(move |chunk| received_for_sink.lock().unwrap().extend(chunk));
        let writer_task = tokio::spawn(async move {
            for byte in 0..4_u8 {
                writer.write_all(&[byte]).await.unwrap();
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        });
        let result = forward_stream(&mut reader, Duration::from_millis(35), &sink).await;
        writer_task.await.unwrap();
        assert_eq!(result.end, StreamEnd::Eof);
        assert_eq!(*received.lock().unwrap(), vec![0, 1, 2, 3]);
        assert!(result.first_chunk_at.is_some());
        assert!(result.last_chunk_at.is_some());
    }

    #[tokio::test]
    async fn watchdog_reports_a_real_gap_after_the_last_chunk() {
        let (mut writer, mut reader) = tokio::io::duplex(64);
        writer.write_all(&[1]).await.unwrap();
        let sink: PreviewSink = Arc::new(|_| {});
        let result = forward_stream(&mut reader, Duration::from_millis(20), &sink).await;
        assert_eq!(result.end, StreamEnd::Stalled);
        assert!(result.first_chunk_at.is_some());
        assert_eq!(result.first_chunk_at, result.last_chunk_at);
    }

    #[tokio::test]
    async fn stderr_is_drained_but_only_the_safe_prefix_is_retained() {
        let (mut writer, reader) = tokio::io::duplex(MAX_PREVIEW_STDERR_BYTES * 2);
        let expected = vec![b'x'; MAX_PREVIEW_STDERR_BYTES * 2];
        writer.write_all(&expected).await.unwrap();
        drop(writer);

        let retained = read_truncated_stderr(reader).await;

        assert_eq!(retained, expected[..MAX_PREVIEW_STDERR_BYTES]);
    }

    #[test]
    #[ignore = "launched by stopping_preview_reaps_the_child_process"]
    fn preview_child_process() {
        println!("PREVIEW_CHILD_PID={}", std::process::id());
        std::thread::sleep(Duration::from_secs(30));
    }

    fn child_pid(output: &[u8]) -> Option<u32> {
        let output = String::from_utf8_lossy(output);
        let marker = "PREVIEW_CHILD_PID=";
        let start = output.find(marker)? + marker.len();
        let end = output[start..]
            .find(|character: char| !character.is_ascii_digit())
            .map_or(output.len(), |offset| start + offset);
        output[start..end].parse().ok()
    }

    #[cfg(unix)]
    fn process_exists(pid: u32) -> bool {
        StdCommand::new("/bin/ps")
            .args(["-p", &pid.to_string()])
            .output()
            .is_ok_and(|output| {
                output.status.success()
                    && output
                        .stdout
                        .windows(pid.to_string().len())
                        .any(|window| window == pid.to_string().as_bytes())
            })
    }

    #[cfg(windows)]
    fn process_exists(pid: u32) -> bool {
        StdCommand::new("tasklist.exe")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .is_ok_and(|output| {
                String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
            })
    }

    #[tokio::test]
    async fn stopping_preview_reaps_the_child_process() {
        let bytes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let bytes_for_sink = bytes.clone();
        let sink: PreviewSink = Arc::new(move |chunk| bytes_for_sink.lock().unwrap().extend(chunk));
        let executable = std::env::current_exe().unwrap();
        let args = [
            "--ignored".to_owned(),
            "--exact".to_owned(),
            "preview::tests::preview_child_process".to_owned(),
            "--nocapture".to_owned(),
        ];
        let backend = AdbScreenrecordPreviewBackend;
        let session = backend
            .start(
                &executable,
                &args,
                sink,
                Arc::new(|_| {}),
                Arc::new(AtomicBool::new(true)),
            )
            .await
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let pid = loop {
            if let Some(pid) = child_pid(&bytes.lock().unwrap()) {
                break pid;
            }
            assert!(
                Instant::now() < deadline,
                "preview child did not report its pid"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert!(process_exists(pid));

        session.stop().await;

        let deadline = Instant::now() + Duration::from_secs(2);
        while process_exists(pid) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!process_exists(pid), "preview child {pid} survived stop");
    }
}
