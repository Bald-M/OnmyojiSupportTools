import { invoke } from '@tauri-apps/api/core'

import type { AppState, ConnectEndpoint, Point, TapReceipt } from '@/types/device'

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

export function tapScreen(point: Point): Promise<TapReceipt> {
  return invoke('tap_screen', { point })
}
