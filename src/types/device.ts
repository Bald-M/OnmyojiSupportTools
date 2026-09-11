export type AdbSource = 'bundled' | 'saved' | 'muMu12' | 'ldPlayer9' | 'path' | 'manual'
export type DeviceStatus = 'online' | 'offline' | 'unauthorized' | 'unknown'

export interface AdbCandidate {
  path: string
  source: AdbSource
  version: string
}

export interface DeviceSummary {
  serial: string
  model: string | null
  status: DeviceStatus
  transport: string | null
}

export interface ConnectEndpoint {
  host: string
  port: number
}

export interface Point {
  x: number
  y: number
}

export interface FrameSummary {
  width: number
  height: number
  deviceSerial: string
  capturedAt: number
}

export interface TapReceipt {
  deviceSerial: string
  point: Point
  completedAt: number
}

export interface AppState {
  adbCandidates: AdbCandidate[]
  selectedAdb: AdbCandidate | null
  devices: DeviceSummary[]
  activeDeviceSerial: string | null
  lastFrame: FrameSummary | null
  lastEndpoint: ConnectEndpoint | null
  previewDeviceSerial: string | null
}

export interface AppError {
  code: string
  message: string
  recovery?: string | null
}
