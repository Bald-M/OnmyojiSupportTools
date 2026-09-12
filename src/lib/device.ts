import { Channel, invoke } from '@tauri-apps/api/core'

import type { ActivityConfig, ActivityRect, ActivitySession, AppState, ClickSettings, ClickTarget, ConnectEndpoint, RecognitionResult, TapReceipt, VisualFeature } from '@/types/device'

export function getAppAppState(): Promise<AppState> {
  return invoke('get_app_app_state')
}

export function setAdbPath(path: string): Promise<AppState> {
  return invoke('set_adb_path', { path })
}

export function refreshDevices(): Promise<AppState> {
  return invoke('refresh_devices')
}

export function connectDevice(endpoint: ConnectEndpoint): Promise<AppState> {
  return invoke('connect_device', { endpoint })
}

export function selectDevice(serial: string): Promise<AppState> {
  return invoke('select_device', { serial })
}

export async function captureScreen(): Promise<Uint8Array> {
  const payload = await invoke<ArrayBuffer | Uint8Array>('capture_screen')
  return payload instanceof Uint8Array ? payload : new Uint8Array(payload)
}

export function tapScreen(target: ClickTarget): Promise<TapReceipt> {
  return invoke('tap_screen', { target })
}

export function setClickSettings(settings: ClickSettings): Promise<AppState> { return invoke('set_click_settings', { settings }) }
export function cancelPendingClick(): Promise<void> { return invoke('cancel_pending_click') }
export function saveActivityConfig(config: ActivityConfig): Promise<ActivityConfig[]> { return invoke('save_activity_config', { config }) }
export function deleteActivityConfig(id: string): Promise<ActivityConfig[]> { return invoke('delete_activity_config', { id }) }
export function calibrateActivityFeature(region: ActivityRect): Promise<VisualFeature> { return invoke('calibrate_activity_feature', { region }) }
export function previewActivityRecognition(configId: string): Promise<RecognitionResult> { return invoke('preview_activity_recognition', { configId }) }
export function startActivity(configId: string, targetRuns: number): Promise<ActivitySession> { return invoke('start_activity', { configId, targetRuns }) }
export function pauseActivity(): Promise<ActivitySession> { return invoke('pause_activity') }
export function resumeActivity(): Promise<ActivitySession> { return invoke('resume_activity') }
export function stopActivity(): Promise<ActivitySession> { return invoke('stop_activity') }
export function advanceActivity(): Promise<ActivitySession> { return invoke('advance_activity') }

export function startPreview(
  onChunk: (chunk: Uint8Array) => void,
  onEnded: () => void,
): Promise<AppState> {
  const channel = new Channel<ArrayBuffer>((payload) => onChunk(new Uint8Array(payload)))
  const endedChannel = new Channel<string>(() => onEnded())
  return invoke('start_preview', { onChunk: channel, onEnded: endedChannel })
}

export function stopPreview(): Promise<AppState> {
  return invoke('stop_preview')
}
