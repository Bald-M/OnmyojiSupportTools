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

export type ClickTarget =
  | { kind: 'point'; x: number; y: number }
  | { kind: 'rect'; left: number; top: number; width: number; height: number }

export interface ClickSettings {
  delayMinimumMs: number
  delayMaximumMs: number
  pointRadius: number
  pressMinimumMs: number
  pressMaximumMs: number
}

export type ActivityKind = 'generic' | 'realmRaid'
export type PageState = 'activityEntry' | 'stageEntry' | 'challenge' | 'opponent' | 'battleReady' | 'battling' | 'reward' | 'defeat' | 'returnChallenge'
export type TaskStatus = 'idle' | 'navigating' | 'ready' | 'starting' | 'battling' | 'rewarding' | 'paused' | 'completed' | 'failed'
export interface ActivityRect { left: number; top: number; width: number; height: number }
export interface VisualFeature { region: ActivityRect; signature: number[] }
export interface FeatureAction { features: VisualFeature[]; action: ActivityRect | null }
export interface RealmRaidOpponent { availableFeatures: VisualFeature[]; action: ActivityRect }
export interface StateProfile { state: PageState; features: VisualFeature[]; action: ActivityRect | null; clickDelay: { minimumMs: number; maximumMs: number } | null; pressDuration: { minimumMs: number; maximumMs: number } | null }
export interface ActivityConfig {
  version: number
  id: string
  name: string
  kind: ActivityKind
  frame: { width: number; height: number; orientation: 'landscape' | 'portrait' }
  states: StateProfile[]
  knownPopups: { name: string; features: VisualFeature[]; closeAction: ActivityRect; clickDelay: { minimumMs: number; maximumMs: number } | null; pressDuration: { minimumMs: number; maximumMs: number } | null }[]
  matching: { threshold: number; minimumMargin: number }
  clickDelay: { minimumMs: number; maximumMs: number }
  pressDuration: { minimumMs: number; maximumMs: number }
  realmRaid: {
    opponents: RealmRaidOpponent[]
    refresh: FeatureAction
    progressRewards: FeatureAction[]
    attackRequirements: VisualFeature[]
    failureLimit: number
    pauseConditions: { name: string; features: VisualFeature[] }[]
  } | null
}
export type SafeAction =
  | { kind: 'page'; value: PageState }
  | { kind: 'realmRaidOpponent'; value: number }
  | { kind: 'realmRaidRefresh' }
  | { kind: 'realmRaidProgressReward'; value: number }
export interface ActivitySession {
  configId: string; status: TaskStatus; currentState: PageState | null; targetRuns: number
  completedRuns: number; nextOpponentIndex: number; attemptedOpponents: number[]; failedOpponents: number[]; consecutiveFailures: number; retryCount: number; pauseReason: { code: string; message: string } | null; lastSafeAction: SafeAction | null
  lastEvent: { kind: string; detail: string } | null
}
export interface RecognitionResult {
  matchedState: PageState | null
  scores: { state: PageState; score: number }[]
  reason: string | null
}

export interface FrameSummary {
  width: number
  height: number
  deviceSerial: string
  capturedAt: number
}

export interface TapReceipt {
  deviceSerial: string
  target: ClickTarget
  delayMs: number
  finalPoint: Point
  pressDurationMs: number
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
  clickSettings: ClickSettings
  activityConfigs: ActivityConfig[]
  activitySession: ActivitySession
  deviceDiscoveryWarnings: string[]
}

export interface AppError {
  code: string
  message: string
  recovery?: string | null
}
