use std::{
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use tokio::{io::AsyncReadExt, process::Command, sync::Mutex, task::JoinHandle, time::timeout};

use crate::device::AppError;

pub(crate) type PreviewSink = Arc<dyn Fn(Vec<u8>) + Send + Sync>;
pub(crate) type PreviewEndSink = Arc<dyn Fn() + Send + Sync>;
const PREVIEW_STALL_TIMEOUT: Duration = Duration::from_secs(8);

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
            .stderr(Stdio::null())
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

        let task = tokio::spawn(async move {
            let mut buffer = vec![0_u8; 64 * 1024];
            loop {
                match timeout(PREVIEW_STALL_TIMEOUT, stdout.read(&mut buffer)).await {
                    Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
                    Ok(Ok(length)) => sink(buffer[..length].to_vec()),
                }
            }
            let _ = child.kill().await;
            let _ = child.wait().await;
            live.store(false, Ordering::Release);
            on_end();
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
    use std::{path::Path, sync::atomic::AtomicBool};

    use super::*;

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
            on_end();
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
                Arc::new(move || ended_for_callback.store(true, Ordering::Release)),
            )
            .await
            .unwrap();

        assert!(ended.load(Ordering::Acquire));
        assert!(controller.active_device_serial().await.is_none());
    }
}
