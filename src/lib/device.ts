import { Channel, invoke } from '@tauri-apps/api/core'

import type { ActivityConfig, ActivityRect, ActivitySession, AppState, ClickSettings, ClickTarget, ConnectEndpoint, RecognitionResult, TapReceipt, VisualFeature } from '@/types/device'

function newTraceId(): string {
  return Array.from(crypto.getRandomValues(new Uint8Array(16)), (byte) => byte.toString(16).padStart(2, '0')).join('')
}

let previewTraceId: string | null = null

export function reportFrontendError(code: string, traceId = newTraceId()): void {
  void invoke('report_frontend_error', { code, traceId }).catch(() => { /* Logging must not interrupt device work. */ })
}

export function reportPreviewError(): void {
  reportFrontendError('PREVIEW_DECODE_FAILED', previewTraceId ?? newTraceId())
}

async function tracedInvoke<T>(command: string, args: Record<string, unknown> = {}, traceId = newTraceId()): Promise<T> {
  try {
    return await invoke<T>(command, { ...args, traceId })
  } catch (error) {
    reportFrontendError('IPC_FAILED', traceId)
    throw error
  }
}

export function diagnosticsStatus(): Promise<boolean> { return invoke('diagnostics_status') }
export function openDiagnosticsDirectory(): Promise<void> { return invoke('open_diagnostics_directory') }
export function exportDiagnostics(path: string): Promise<void> { return invoke('export_diagnostics', { path }) }

export type PreviewEndCode = 'PREVIEW_STREAM_EOF' | 'PREVIEW_READ_FAILED' | 'PREVIEW_STREAM_STALLED' | 'PREVIEW_PROCESS_EXITED'

export interface PreviewEnd {
  code: PreviewEndCode
  exitCode: number | null
  firstChunkAt: number | null
  lastChunkAt: number | null
  stderr: string | null
}

export function getAppAppState(): Promise<AppState> {
  return tracedInvoke('get_app_app_state')
}

export function setAdbPath(path: string): Promise<AppState> {
  return tracedInvoke('set_adb_path', { path })
}

export function refreshDevices(): Promise<AppState> {
  return tracedInvoke('refresh_devices')
}

export function connectDevice(endpoint: ConnectEndpoint): Promise<AppState> {
  return tracedInvoke('connect_device', { endpoint })
}

export function selectDevice(serial: string): Promise<AppState> {
  return tracedInvoke('select_device', { serial })
}

export async function captureScreen(): Promise<Uint8Array> {
  const payload = await tracedInvoke<ArrayBuffer | Uint8Array>('capture_screen')
  return payload instanceof Uint8Array ? payload : new Uint8Array(payload)
}

export function tapScreen(target: ClickTarget): Promise<TapReceipt> {
  return tracedInvoke('tap_screen', { target })
}

export function setClickSettings(settings: ClickSettings): Promise<AppState> { return tracedInvoke('set_click_settings', { settings }) }
export function cancelPendingClick(): Promise<void> { return tracedInvoke('cancel_pending_click') }
export function saveActivityConfig(config: ActivityConfig): Promise<ActivityConfig[]> { return tracedInvoke('save_activity_config', { config }) }
export function deleteActivityConfig(id: string): Promise<ActivityConfig[]> { return tracedInvoke('delete_activity_config', { id }) }
export function calibrateActivityFeature(region: ActivityRect): Promise<VisualFeature> { return tracedInvoke('calibrate_activity_feature', { region }) }
export function previewActivityRecognition(configId: string): Promise<RecognitionResult> { return tracedInvoke('preview_activity_recognition', { configId }) }
export function startActivity(configId: string, targetRuns: number): Promise<ActivitySession> { return tracedInvoke('start_activity', { configId, targetRuns }) }
export function pauseActivity(): Promise<ActivitySession> { return tracedInvoke('pause_activity') }
export function resumeActivity(): Promise<ActivitySession> { return tracedInvoke('resume_activity') }
export function stopActivity(): Promise<ActivitySession> { return tracedInvoke('stop_activity') }
export function advanceActivity(): Promise<ActivitySession> { return tracedInvoke('advance_activity') }

export function startPreview(
  onChunk: (chunk: Uint8Array) => void,
  onEnded: (end: PreviewEnd) => void,
): Promise<AppState> {
  const traceId = newTraceId()
  previewTraceId = traceId
  const channel = new Channel<ArrayBuffer>((payload) => onChunk(new Uint8Array(payload)))
  const endedChannel = new Channel<PreviewEnd>((end) => onEnded(end))
  return tracedInvoke<AppState>('start_preview', { onChunk: channel, onEnded: endedChannel }, traceId)
    .catch((error) => { previewTraceId = null; throw error })
}

export function stopPreview(): Promise<AppState> {
  return tracedInvoke<AppState>('stop_preview').finally(() => { previewTraceId = null })
}
