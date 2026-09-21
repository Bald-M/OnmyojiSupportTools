<script setup lang="ts">
import {
  Camera,
  CheckCircle2,
  CircleAlert,
  FolderOpen,
  MonitorSmartphone,
  MousePointerClick,
  PlugZap,
  RefreshCw,
  Video,
  Square,
  Move,
} from '@lucide/vue'
import { open } from '@tauri-apps/plugin-dialog'
import { computed, onBeforeUnmount, onMounted, reactive, ref, shallowRef } from 'vue'

import { calculateContainedImageRect, mapClientPointToImage } from './lib/coordinates'
import {
  captureScreen,
  connectDevice,
  getAppAppState,
  refreshDevices,
  selectDevice,
  setAdbPath,
  startPreview,
  stopPreview,
  tapScreen,
  cancelPendingClick,
  advanceActivity,
  calibrateActivityFeature,
  pauseActivity,
  previewActivityRecognition,
  resumeActivity,
  saveActivityConfig,
  startActivity,
  stopActivity,
  setClickSettings,
  deleteActivityConfig,
  type PreviewEnd,
} from './lib/device'
import { H264CanvasDecoder } from './lib/h264'
import type { ActivityConfig, AdbSource, AppError, AppState, ClickTarget, DeviceStatus, PageState, RecognitionResult } from './types/device'

type Operation = 'initialize' | 'adb' | 'devices' | 'connect' | 'capture' | 'preview' | 'tap' | 'activity'
type StatusTone = 'neutral' | 'success' | 'error'

const emptyState: AppState = {
  adbCandidates: [],
  selectedAdb: null,
  devices: [],
  activeDeviceSerial: null,
  lastFrame: null,
  lastEndpoint: null,
  previewDeviceSerial: null,
  clickSettings: { delayMinimumMs: 300, delayMaximumMs: 900, pointRadius: 6, pressMinimumMs: 45, pressMaximumMs: 120 },
  activityConfigs: [],
  activitySession: { configId: '', status: 'idle', currentState: null, targetRuns: 0, completedRuns: 0, nextOpponentIndex: 0, attemptedOpponents: [], failedOpponents: [], consecutiveFailures: 0, retryCount: 0, pauseReason: null, lastSafeAction: null, lastEvent: null },
}

const appState = ref<AppState>(emptyState)
const busy = ref<Operation | null>('initialize')
const screenshotUrl = shallowRef<string | null>(null)
const screenshotStage = ref<HTMLElement | null>(null)
const previewCanvas = ref<HTMLCanvasElement | null>(null)
const imageSize = reactive({ width: 0, height: 0 })
const stageSize = reactive({ width: 0, height: 0 })
const selectedPoint = reactive<{ x: number | null; y: number | null }>({
  x: null,
  y: null,
})
const selectedTarget = ref<ClickTarget | null>(null)
type ResizeHandle = 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w' | 'nw'
const resizeHandles: ResizeHandle[] = ['n', 'ne', 'e', 'se', 's', 'sw', 'w', 'nw']
const selectionMode = ref<'point' | 'rect'>('point')
let edit: { pointerId: number; start: { x: number; y: number }; original: Extract<ClickTarget, { kind: 'rect' }> | null; handle: ResizeHandle | 'move' | 'create'; clientStart: { x: number; y: number } } | null = null
let suppressStageClick = false
const endpointHost = ref('127.0.0.1')
const endpointPort = ref('')
const status = reactive<{ tone: StatusTone; message: string; recovery: string | null }>({
  tone: 'neutral',
  message: '正在初始化设备环境…',
  recovery: null,
})
let stageResizeObserver: ResizeObserver | null = null
let previewDecoder: H264CanvasDecoder | null = null
let previewFailurePending = false
let activityTimer: number | null = null
let tapCountdownTimer: number | null = null
const tapCountdown = ref(0)
const activityName = ref('结界突破')
const popupName = ref('悬赏邀请')
const selectedConfigId = ref('')
const selectedCalibrationState = ref<PageState>('activityEntry')
const targetRuns = ref(1)
const resumePreview = ref<RecognitionResult | null>(null)
const recognitionPreview = ref<RecognitionResult | null>(null)
const overrideDelayMinimum = ref<number | null>(null)
const overrideDelayMaximum = ref<number | null>(null)
const overridePressMinimum = ref<number | null>(null)
const overridePressMaximum = ref<number | null>(null)
const draftConfig = ref<ActivityConfig | null>(null)
const pageLabels: Record<PageState, string> = {
  activityEntry: '庭院探索入口',
  stageEntry: '探索地图结界突破入口',
  challenge: '结界突破九宫格',
  opponent: '对手详情',
  battleReady: '战斗准备',
  battling: '战斗中',
  reward: '胜利奖励',
  defeat: '失败结算',
  returnChallenge: '返回挑战页',
}
const realmRaidStates: PageState[] = ['activityEntry', 'stageEntry', 'challenge', 'opponent', 'battleReady', 'battling', 'reward', 'defeat']
const genericStates: PageState[] = ['activityEntry', 'stageEntry', 'challenge', 'battling', 'reward', 'returnChallenge']
const calibrationStates = computed(() => draftConfig.value?.kind === 'generic' ? genericStates : realmRaidStates)
const targetRunsError = computed(() => {
  const value = Number(targetRuns.value)
  return Number.isInteger(value) && value >= 1 && value <= 30 ? null : '请输入 1 到 30 的整数。'
})
const adbSourceLabels: Record<AdbSource, string> = {
  bundled: '应用内置',
  saved: '上次使用',
  muMu12: 'MuMu 12',
  ldPlayer9: '雷电 9',
  path: '系统 PATH',
  manual: '手动选择',
}
const deviceStatusLabels: Record<DeviceStatus, string> = {
  online: '在线',
  offline: '离线',
  unauthorized: '未授权',
  unknown: '未知',
}

const onlineDevices = computed(() =>
  appState.value.devices.filter((device) => device.status === 'online'),
)
const canCapture = computed(
  () => Boolean(appState.value.selectedAdb && appState.value.activeDeviceSerial) && !appState.value.previewDeviceSerial && !busy.value,
)
const hasVisualFrame = computed(() => Boolean(screenshotUrl.value || appState.value.previewDeviceSerial))
const hasSelectedPoint = computed(
  () =>
    Boolean(hasVisualFrame.value && appState.value.activeDeviceSerial) &&
    imageSize.width > 0 &&
    Number.isInteger(selectedPoint.x) &&
    Number.isInteger(selectedPoint.y) &&
    Number(selectedPoint.x) >= 0 &&
    Number(selectedPoint.y) >= 0 &&
    Number(selectedPoint.x) < imageSize.width &&
    Number(selectedPoint.y) < imageSize.height,
)
const canTap = computed(() => Boolean((hasSelectedPoint.value || selectedTarget.value?.kind === 'rect') && !busy.value))
const markerStyle = computed(() => {
  if (!hasSelectedPoint.value || stageSize.width <= 0 || stageSize.height <= 0) {
    return { display: 'none' }
  }
  const fitted = calculateContainedImageRect(
    { left: 0, top: 0, width: stageSize.width, height: stageSize.height },
    imageSize,
  )
  const xRatio = imageSize.width <= 1 ? 0 : Number(selectedPoint.x) / (imageSize.width - 1)
  const yRatio = imageSize.height <= 1 ? 0 : Number(selectedPoint.y) / (imageSize.height - 1)
  return {
    left: `${fitted.left + fitted.width * xRatio}px`,
    top: `${fitted.top + fitted.height * yRatio}px`,
  }
})
const selectionStyle = computed(() => {
  if (selectedTarget.value?.kind !== 'rect' || !stageSize.width || !stageSize.height) return { display: 'none' }
  const fitted = calculateContainedImageRect({ left: 0, top: 0, width: stageSize.width, height: stageSize.height }, imageSize)
  const target = selectedTarget.value
  return { left: `${fitted.left + target.left / imageSize.width * fitted.width}px`, top: `${fitted.top + target.top / imageSize.height * fitted.height}px`, width: `${target.width / imageSize.width * fitted.width}px`, height: `${target.height / imageSize.height * fitted.height}px` }
})

const selectionControlsStyle = computed(() => {
  if (selectedTarget.value?.kind !== 'rect' || !stageSize.width || !stageSize.height) return { display: 'none' }
  const fitted = calculateContainedImageRect({ left: 0, top: 0, width: stageSize.width, height: stageSize.height }, imageSize)
  const target = selectedTarget.value
  const width = Math.min(stageSize.width, Math.max(96, target.width / imageSize.width * fitted.width))
  const height = Math.min(stageSize.height, Math.max(96, target.height / imageSize.height * fitted.height))
  const left = Math.max(0, Math.min(stageSize.width - width, fitted.left + (target.left + target.width / 2) / imageSize.width * fitted.width - width / 2))
  const top = Math.max(0, Math.min(stageSize.height - height, fitted.top + (target.top + target.height / 2) / imageSize.height * fitted.height - height / 2))
  return { left: `${left}px`, top: `${top}px`, width: `${width}px`, height: `${height}px` }
})

function setStatus(tone: StatusTone, message: string, recovery: string | null = null) {
  status.tone = tone
  status.message = message
  status.recovery = recovery
}

function normalizeError(error: unknown): AppError {
  if (error && typeof error === 'object' && 'message' in error) {
    const candidate = error as Partial<AppError>
    return {
      code: candidate.code ?? 'UNKNOWN',
      message: String(candidate.message),
      recovery: candidate.recovery ?? '请重试；如果问题持续，请在 Issue 中附上环境信息。',
    }
  }
  return {
    code: 'UNKNOWN',
    message: typeof error === 'string' ? error : '发生未知错误',
    recovery: '请重试；如果问题持续，请在 Issue 中附上环境信息。',
  }
}

async function runOperation<T>(
  operation: Operation,
  task: () => Promise<T>,
  onSuccess: (result: T) => void,
) {
  if (busy.value) return
  busy.value = operation
  try {
    onSuccess(await task())
  } catch (error) {
    const appError = normalizeError(error)
    setStatus('error', appError.message, appError.recovery ?? null)
  } finally {
    busy.value = null
  }
}

function clearScreenshot() {
  endSelectionEdit()
  if (screenshotUrl.value) {
    URL.revokeObjectURL(screenshotUrl.value)
  }
  screenshotUrl.value = null
  imageSize.width = 0
  imageSize.height = 0
  selectedPoint.x = null
  selectedPoint.y = null
  selectedTarget.value = null
}

function ensureDraft(): ActivityConfig | null {
  if (draftConfig.value) return draftConfig.value
  if (!imageSize.width || !imageSize.height) return null
  draftConfig.value = {
    version: 1, id: window.crypto.randomUUID(), name: activityName.value, kind: 'realmRaid',
    frame: { width: imageSize.width, height: imageSize.height, orientation: imageSize.width >= imageSize.height ? 'landscape' : 'portrait' },
    states: realmRaidStates.map((state) => ({ state, features: [], action: null, clickDelay: null, pressDuration: null })),
    knownPopups: [], matching: { threshold: 0.9, minimumMargin: 0.08 },
    clickDelay: { minimumMs: 300, maximumMs: 900 }, pressDuration: { minimumMs: 45, maximumMs: 120 },
    realmRaid: {
      opponents: [],
      refresh: { features: [], action: null },
      progressRewards: [],
      attackRequirements: [],
      failureLimit: 3,
      pauseConditions: [],
    },
  }
  return draftConfig.value
}

function selectedRect() {
  return selectedTarget.value?.kind === 'rect' ? { left: selectedTarget.value.left, top: selectedTarget.value.top, width: selectedTarget.value.width, height: selectedTarget.value.height } : null
}

function createRectFromPoint() {
  endSelectionEdit()
  if (!imageSize.width || !imageSize.height) return
  const x = Math.max(0, Math.min(imageSize.width - 1, Number.isInteger(selectedPoint.x) ? Number(selectedPoint.x) : 0))
  const y = Math.max(0, Math.min(imageSize.height - 1, Number.isInteger(selectedPoint.y) ? Number(selectedPoint.y) : 0))
  selectedTarget.value = { kind: 'rect', left: x, top: y, width: Math.min(40, imageSize.width - x), height: Math.min(40, imageSize.height - y) }
  selectedPoint.x = null; selectedPoint.y = null
}

function normalizeKeyboardRect() {
  endSelectionEdit()
  const target = selectedTarget.value
  if (target?.kind !== 'rect') return
  target.left = Math.max(0, Math.min(Math.trunc(Number(target.left) || 0), imageSize.width - 1))
  target.top = Math.max(0, Math.min(Math.trunc(Number(target.top) || 0), imageSize.height - 1))
  target.width = Math.max(1, Math.min(Math.trunc(Number(target.width) || 1), imageSize.width - target.left))
  target.height = Math.max(1, Math.min(Math.trunc(Number(target.height) || 1), imageSize.height - target.top))
}

function addCalibrationFeature() {
  const rect = selectedRect(); const draft = ensureDraft()
  if (!rect || !draft) { setStatus('error', '请先在当前截图上框选识别区域。'); return }
  void runOperation('activity', () => calibrateActivityFeature(rect), (feature) => {
    draft.states.find((item) => item.state === selectedCalibrationState.value)?.features.push(feature)
    setStatus('success', `已为${pageLabels[selectedCalibrationState.value]}添加不可逆视觉特征。`)
  })
}

function setCalibrationAction() {
  const rect = selectedRect(); const draft = ensureDraft()
  if (!rect || !draft) { setStatus('error', '请先框选安全动作区域。'); return }
  const profile = draft.states.find((item) => item.state === selectedCalibrationState.value)
  if (profile) profile.action = rect
  setStatus('success', `已设置${pageLabels[selectedCalibrationState.value]}动作区域。`)
}

function addRealmRaidOpponentAction() {
  const rect = selectedRect(); const draft = ensureDraft()
  if (!rect || draft?.kind !== 'realmRaid' || !draft.realmRaid) { setStatus('error', '请先框选九宫格中的对手区域。'); return }
  if (draft.realmRaid.opponents.length >= 9) { setStatus('error', '九个对手区域已经全部设置。'); return }
  draft.realmRaid.opponents.push({ action: rect, availableFeatures: [] })
  setStatus('success', `已添加第 ${draft.realmRaid.opponents.length} 个对手区域；请继续添加它的可挑战特征。`)
}

function setRealmRaidRefreshAction() {
  const rect = selectedRect(); const draft = ensureDraft()
  if (!rect || draft?.kind !== 'realmRaid' || !draft.realmRaid) { setStatus('error', '请先框选刷新按钮。'); return }
  draft.realmRaid.refresh.action = rect
  setStatus('success', '已设置九宫格刷新区域。')
}

function addOpponentAvailabilityFeature() {
  const rect = selectedRect(); const draft = ensureDraft(); const opponents = draft?.realmRaid?.opponents
  if (!rect || draft?.kind !== 'realmRaid' || !opponents?.length) { setStatus('error', '请先添加一个对手区域，再框选其可挑战标志。'); return }
  void runOperation('activity', () => calibrateActivityFeature(rect), (feature) => {
    opponents[opponents.length - 1]!.availableFeatures.push(feature)
    setStatus('success', `已为第 ${opponents.length} 个对手添加可挑战特征。`)
  })
}

function addRefreshAvailabilityFeature() {
  const rect = selectedRect(); const draft = ensureDraft()
  if (!rect || draft?.kind !== 'realmRaid' || !draft.realmRaid) { setStatus('error', '请在刷新可用时框选稳定标志。'); return }
  void runOperation('activity', () => calibrateActivityFeature(rect), (feature) => {
    draft.realmRaid!.refresh.features.push(feature)
    setStatus('success', '已添加刷新可用特征。')
  })
}

function addProgressRewardAction() {
  const rect = selectedRect(); const draft = ensureDraft(); const rewards = draft?.realmRaid?.progressRewards
  if (!rect || draft?.kind !== 'realmRaid' || !rewards) { setStatus('error', '请先框选进度奖励区域。'); return }
  if (rewards.length >= 3) { setStatus('error', '3/6/9 三个奖励区域已经全部设置。'); return }
  rewards.push({ action: rect, features: [] })
  setStatus('success', `已添加第 ${rewards.length} 个进度奖励；请继续添加其可领取特征。`)
}

function addProgressRewardFeature() {
  const rect = selectedRect(); const draft = ensureDraft(); const rewards = draft?.realmRaid?.progressRewards
  if (!rect || draft?.kind !== 'realmRaid' || !rewards?.length) { setStatus('error', '请先添加一个进度奖励区域。'); return }
  void runOperation('activity', () => calibrateActivityFeature(rect), (feature) => {
    rewards[rewards.length - 1]!.features.push(feature)
    setStatus('success', `已为第 ${rewards.length} 个进度奖励添加可领取特征。`)
  })
}

function addAttackRequirement() {
  const rect = selectedRect(); const draft = ensureDraft()
  if (!rect || draft?.kind !== 'realmRaid' || !draft.realmRaid) { setStatus('error', '请在对手详情页框选进攻门禁特征。'); return }
  void runOperation('activity', () => calibrateActivityFeature(rect), (feature) => {
    draft.realmRaid!.attackRequirements.push(feature)
    setStatus('success', `已添加第 ${draft.realmRaid!.attackRequirements.length} 项进攻门禁特征。`)
  })
}

function addPauseCondition() {
  const rect = selectedRect(); const draft = ensureDraft(); const name = popupName.value.trim()
  if (!rect || draft?.kind !== 'realmRaid' || !draft.realmRaid || !name) { setStatus('error', '请输入暂停条件名称并框选稳定特征。'); return }
  void runOperation('activity', () => calibrateActivityFeature(rect), (feature) => {
    let condition = draft.realmRaid!.pauseConditions.find((item) => item.name === name)
    if (!condition) { condition = { name, features: [] }; draft.realmRaid!.pauseConditions.push(condition) }
    condition.features.push(feature)
    setStatus('success', `已添加“${name}”安全暂停条件。`)
  })
}

function addPopupFeature() {
  const rect = selectedRect(); const draft = ensureDraft(); const name = popupName.value.trim()
  if (!rect || !draft || !name) { setStatus('error', '请输入弹窗名称并框选稳定识别区域。'); return }
  void runOperation('activity', () => calibrateActivityFeature(rect), (feature) => {
    let popup = draft.knownPopups.find((item) => item.name === name)
    if (!popup) { popup = { name, features: [], closeAction: rect, clickDelay: null, pressDuration: null }; draft.knownPopups.push(popup) }
    popup.features.push(feature)
    setStatus('success', `已为已知弹窗“${name}”添加识别区域。`)
  })
}

function setPopupCloseAction() {
  const rect = selectedRect(); const draft = ensureDraft(); const name = popupName.value.trim()
  if (!rect || !draft || !name) { setStatus('error', '请输入弹窗名称并框选关闭区域。'); return }
  let popup = draft.knownPopups.find((item) => item.name === name)
  if (!popup) { popup = { name, features: [], closeAction: rect, clickDelay: null, pressDuration: null }; draft.knownPopups.push(popup) }
  popup.closeAction = rect
  setStatus('success', `已设置“${name}”的唯一关闭区域。`)
}

function saveCalibration() {
  const draft = ensureDraft(); if (!draft) return
  draft.name = activityName.value.trim()
  void runOperation('activity', () => saveActivityConfig(draft), (configs) => {
    appState.value = { ...appState.value, activityConfigs: configs }
    selectedConfigId.value = draft.id
    setStatus('success', '活动配置已保存；原始截图与裁剪图未持久化。')
  })
}

function handleConfigSelection() {
  const config = appState.value.activityConfigs.find((item) => item.id === selectedConfigId.value)
  if (!config) return
  draftConfig.value = JSON.parse(JSON.stringify(config)) as ActivityConfig
  activityName.value = config.name
}

function newCalibration() {
  draftConfig.value = null
  selectedConfigId.value = ''
  activityName.value = '结界突破'
  recognitionPreview.value = null
  if (imageSize.width && imageSize.height) ensureDraft()
  setStatus('neutral', '已开始结界突破配置；请依次校准八个页面状态、九个对手区域和刷新按钮。')
}

function deleteCalibration() {
  if (!selectedConfigId.value) return
  if (!window.confirm('删除此本机活动配置？任务配置和不可逆特征将无法恢复。')) return
  void runOperation('activity', () => deleteActivityConfig(selectedConfigId.value), (configs) => {
    appState.value = { ...appState.value, activityConfigs: configs }
    selectedConfigId.value = ''
    draftConfig.value = null
    recognitionPreview.value = null
    setStatus('success', '活动配置已删除。')
  })
}

function previewCalibration() {
  if (!selectedConfigId.value) return
  void runOperation('activity', () => previewActivityRecognition(selectedConfigId.value), (result) => {
    recognitionPreview.value = result
    setStatus('neutral', result.matchedState ? `唯一识别：${pageLabels[result.matchedState]}` : `未产生动作建议：${result.reason}`)
  })
}

function applyActionTimingOverride() {
  const draft = ensureDraft(); const profile = draft?.states.find((item) => item.state === selectedCalibrationState.value)
  if (!profile) return
  profile.clickDelay = overrideDelayMinimum.value == null || overrideDelayMaximum.value == null ? null : { minimumMs: overrideDelayMinimum.value, maximumMs: overrideDelayMaximum.value }
  profile.pressDuration = overridePressMinimum.value == null || overridePressMaximum.value == null ? null : { minimumMs: overridePressMinimum.value, maximumMs: overridePressMaximum.value }
  setStatus('success', `已更新${pageLabels[selectedCalibrationState.value]}的逐动作时序覆盖。`)
}

function applyPopupTimingOverride() {
  const popup = draftConfig.value?.knownPopups.find((item) => item.name === popupName.value.trim())
  if (!popup) { setStatus('error', '请先添加该已知弹窗。'); return }
  popup.clickDelay = overrideDelayMinimum.value == null || overrideDelayMaximum.value == null ? null : { minimumMs: overrideDelayMinimum.value, maximumMs: overrideDelayMaximum.value }
  popup.pressDuration = overridePressMinimum.value == null || overridePressMaximum.value == null ? null : { minimumMs: overridePressMinimum.value, maximumMs: overridePressMaximum.value }
  setStatus('success', `已更新“${popup.name}”的关闭时序覆盖。`)
}

function scheduleActivityStep() {
  if (activityTimer) window.clearTimeout(activityTimer)
  const active = !['idle', 'paused', 'completed', 'failed'].includes(appState.value.activitySession.status)
  if (!active) return
  activityTimer = window.setTimeout(async () => {
    try { appState.value.activitySession = await advanceActivity() }
    catch (error) { const appError = normalizeError(error); setStatus('error', appError.message, appError.recovery ?? null); return }
    scheduleActivityStep()
  }, 1200)
}

function handleStartActivity() {
  if (!selectedConfigId.value) { setStatus('error', '请先选择活动配置。'); return }
  if (!Number.isInteger(Number(targetRuns.value)) || Number(targetRuns.value) < 1 || Number(targetRuns.value) > 30) {
    setStatus('error', '目标成功次数必须是 1 到 30 的整数。')
    return
  }
  void runOperation('activity', () => startActivity(selectedConfigId.value, Number(targetRuns.value)), (session) => {
    appState.value.activitySession = session; resumePreview.value = null; scheduleActivityStep(); setStatus('success', '活动任务已开始。')
  })
}

function handlePauseActivity() { void runOperation('activity', pauseActivity, (session) => { appState.value.activitySession = session; scheduleActivityStep(); setStatus('neutral', '任务已暂停，不会继续发送输入。') }) }
function handleStopActivity() { void runOperation('activity', stopActivity, (session) => { appState.value.activitySession = session; scheduleActivityStep(); setStatus('neutral', '任务已停止。') }) }
function handleResumePreview() {
  void runOperation('activity', async () => {
    const bytes = await captureScreen()
    clearScreenshot()
    screenshotUrl.value = URL.createObjectURL(new Blob([bytes.slice().buffer], { type: 'image/png' }))
    return previewActivityRecognition(appState.value.activitySession.configId)
  }, (result) => { resumePreview.value = result; setStatus('neutral', result.matchedState ? `当前识别：${pageLabels[result.matchedState]}。请确认后恢复。` : '当前页面无法唯一识别，不能恢复。') })
}
function handleResumeConfirm() { void runOperation('activity', resumeActivity, (session) => { appState.value.activitySession = session; resumePreview.value = null; scheduleActivityStep(); setStatus('success', '已确认恢复任务。') }) }

function closePreviewDecoder() {
  previewDecoder?.close()
  previewDecoder = null
}

async function recoverFromPreviewFailure(message: string) {
  if (previewFailurePending) return
  previewFailurePending = true
  try {
    applyState(await stopPreview())
  } catch {
    appState.value = { ...appState.value, previewDeviceSerial: null, lastFrame: null }
  } finally {
    closePreviewDecoder()
    clearScreenshot()
    previewFailurePending = false
    setStatus('error', message, '实时预览已停止，请继续使用“刷新截图”。')
  }
}

function applyState(nextState: AppState) {
  const deviceChanged = appState.value.activeDeviceSerial !== nextState.activeDeviceSerial
  const adbChanged = appState.value.selectedAdb?.path !== nextState.selectedAdb?.path
  const previewStopped = Boolean(appState.value.previewDeviceSerial && !nextState.previewDeviceSerial)
  appState.value = nextState
  if (deviceChanged || adbChanged || previewStopped) {
    clearScreenshot()
    closePreviewDecoder()
  }
  if (nextState.lastEndpoint) {
    endpointHost.value = nextState.lastEndpoint.host
    endpointPort.value = String(nextState.lastEndpoint.port)
  }
}

async function initialize() {
  try {
    const state = await getAppAppState()
    applyState(state)
    setStatus(
      'neutral',
      state.selectedAdb
        ? '设备环境已就绪。'
        : '未检测到可用的 ADB，请手动选择 adb.exe。',
    )
  } catch (error) {
    const appError = normalizeError(error)
    setStatus('error', appError.message, appError.recovery ?? null)
  } finally {
    busy.value = null
  }
}

async function chooseAdb() {
  let result: string | string[] | null
  try {
    result = await open({
      multiple: false,
      directory: false,
      filters: [{ name: 'Android Debug Bridge', extensions: ['exe'] }],
    })
  } catch (error) {
    const appError = normalizeError(error)
    setStatus('error', appError.message, appError.recovery ?? null)
    return
  }
  if (typeof result !== 'string') return
  await runOperation('adb', () => setAdbPath(result), (state) => {
    applyState(state)
    setStatus('success', 'ADB 已验证，设备列表已刷新。')
  })
}

function handleAdbChange(event: Event) {
  const path = (event.target as HTMLSelectElement).value
  if (!path || path === appState.value.selectedAdb?.path) return
  void runOperation('adb', () => setAdbPath(path), (state) => {
    applyState(state)
    setStatus('success', '已切换 ADB。')
  })
}

function handleDeviceChange(event: Event) {
  const serial = (event.target as HTMLSelectElement).value
  if (!serial || serial === appState.value.activeDeviceSerial) return
  void runOperation('devices', () => selectDevice(serial), (state) => {
    applyState(state)
    setStatus('success', `已选择设备 ${serial}。`)
  })
}

function handleRefreshDevices() {
  void runOperation('devices', refreshDevices, (state) => {
    applyState(state)
    setStatus('success', `设备列表已刷新，发现 ${state.devices.length} 台设备。`)
  })
}

function handleConnect() {
  const port = Number(endpointPort.value)
  if (!endpointHost.value.trim() || !Number.isInteger(port) || port < 1 || port > 65535) {
    setStatus('error', '请输入有效的 IP 地址和端口。', '端口范围为 1 到 65535。')
    return
  }
  void runOperation(
    'connect',
    () => connectDevice({ host: endpointHost.value.trim(), port }),
    (state) => {
      applyState(state)
      setStatus('success', `已请求连接 ${endpointHost.value.trim()}:${port}。`)
    },
  )
}

function handleCapture() {
  void runOperation('capture', captureScreen, (bytes) => {
    clearScreenshot()
    const safeBytes = bytes.slice()
    screenshotUrl.value = URL.createObjectURL(
      new Blob([safeBytes.buffer], { type: 'image/png' }),
    )
    setStatus('success', '截图已获取，请在画面中选择坐标。')
  })
}

function handlePreviewChunk(chunk: Uint8Array) {
  if (!previewCanvas.value) return
  try {
    previewDecoder ??= new H264CanvasDecoder(previewCanvas.value, (error) => {
      void recoverFromPreviewFailure(`实时预览解码失败：${error.message}`)
    })
    previewDecoder.push(chunk)
  } catch (error) {
    const message = error instanceof Error ? error.message : '实时预览解码失败'
    void recoverFromPreviewFailure(message)
  }
}

function previewEndMessage(end: PreviewEnd): string {
  const detail = end.stderr ? `：${end.stderr}` : ''
  switch (end.code) {
    case 'PREVIEW_STREAM_STALLED': return '实时预览码流已连续 8 秒没有数据。'
    case 'PREVIEW_READ_FAILED': return `读取实时预览码流失败${detail}`
    case 'PREVIEW_PROCESS_EXITED': return `实时预览进程异常退出${end.exitCode === null ? '' : `（退出码 ${end.exitCode}）`}${detail}`
    case 'PREVIEW_STREAM_EOF': return `实时预览码流已结束${detail}`
  }
}

function handlePreviewToggle() {
  if (appState.value.previewDeviceSerial) {
    void runOperation('preview', stopPreview, (state) => {
      applyState(state)
      closePreviewDecoder()
      clearScreenshot()
      setStatus('neutral', '实时预览已停止，可继续使用静态截图。')
    })
    return
  }
  clearScreenshot()
  imageSize.width = 1280
  imageSize.height = 720
  void runOperation('preview', () => startPreview(handlePreviewChunk, (end) => {
    void recoverFromPreviewFailure(previewEndMessage(end))
  }), (state) => {
    applyState(state)
    syncStageSize()
    setStatus('success', '实时预览原型已启动；点击画面只会选择坐标。')
  })
}

function handleImageLoad(event: Event) {
  const image = event.target as HTMLImageElement
  imageSize.width = image.naturalWidth
  imageSize.height = image.naturalHeight
  syncStageSize()
}

function syncStageSize() {
  if (!screenshotStage.value) return
  const rect = screenshotStage.value.getBoundingClientRect()
  stageSize.width = rect.width
  stageSize.height = rect.height
}

function selectPoint(event: MouseEvent) {
  if (!hasVisualFrame.value || busy.value || !screenshotStage.value) return
  const point = mapClientPointToImage({ x: event.clientX, y: event.clientY }, screenshotStage.value.getBoundingClientRect(), imageSize)
  if (!point) return
  selectedPoint.x = point.x
  selectedPoint.y = point.y
  selectedTarget.value = { kind: 'point', ...point }
  syncStageSize()
}

function handleStageClick(event: MouseEvent) {
  if (suppressStageClick) { suppressStageClick = false; return }
  if (selectionMode.value === 'point' && selectedTarget.value?.kind !== 'rect') selectPoint(event)
}

function endSelectionEdit() {
  const pointerId = edit?.pointerId
  edit = null
  if (pointerId !== undefined && screenshotStage.value?.hasPointerCapture?.(pointerId)) screenshotStage.value.releasePointerCapture(pointerId)
}

function handleStagePointerDown(event: PointerEvent) {
  if (event.button !== 0 || busy.value || edit || !hasVisualFrame.value || !screenshotStage.value || !imageSize.width) return
  const container = screenshotStage.value.getBoundingClientRect()
  const targetElement = event.target as HTMLElement
  const handle = targetElement.closest<HTMLElement>('[data-resize-handle]')?.dataset.resizeHandle as ResizeHandle | undefined
  const moving = Boolean(targetElement.closest('.selection-rect, [data-selection-move]'))
  if (!mapClientPointToImage({ x: event.clientX, y: event.clientY }, container, imageSize) && !handle) return
  const fitted = calculateContainedImageRect(container, imageSize)
  const start = { x: (event.clientX - fitted.left) / fitted.width * imageSize.width, y: (event.clientY - fitted.top) / fitted.height * imageSize.height }
  const original = selectedTarget.value?.kind === 'rect' ? { ...selectedTarget.value } : null
  edit = { pointerId: event.pointerId, start, original, handle: handle ?? (moving && original ? 'move' : 'create'), clientStart: { x: event.clientX, y: event.clientY } }
  suppressStageClick = false
  screenshotStage.value.setPointerCapture?.(event.pointerId)
  event.preventDefault()
  if (selectionMode.value === 'rect' && edit.handle === 'create') handleStagePointerMove(event)
}

function handleStagePointerMove(event: PointerEvent) {
  if (!edit || edit.pointerId !== event.pointerId || busy.value || !screenshotStage.value) return
  const fitted = calculateContainedImageRect(screenshotStage.value.getBoundingClientRect(), imageSize)
  if (!fitted.width || !fitted.height) return
  const x = (event.clientX - fitted.left) / fitted.width * imageSize.width
  const y = (event.clientY - fitted.top) / fitted.height * imageSize.height
  const clamp = (value: number, maximum: number) => Math.max(0, Math.min(value, maximum))
  const original = edit.original
  let left: number, top: number, right: number, bottom: number
  if (original && edit.handle === 'move') {
    left = clamp(original.left + Math.round(x - edit.start.x), imageSize.width - original.width)
    top = clamp(original.top + Math.round(y - edit.start.y), imageSize.height - original.height)
    right = left + original.width; bottom = top + original.height
  } else if (original && edit.handle !== 'create') {
    left = original.left; top = original.top; right = left + original.width; bottom = top + original.height
    if (edit.handle.includes('w')) left = clamp(original.left + Math.round(x - edit.start.x), right - 1)
    if (edit.handle.includes('e')) right = Math.max(left + 1, clamp(original.left + original.width + Math.round(x - edit.start.x), imageSize.width))
    if (edit.handle.includes('n')) top = clamp(original.top + Math.round(y - edit.start.y), bottom - 1)
    if (edit.handle.includes('s')) bottom = Math.max(top + 1, clamp(original.top + original.height + Math.round(y - edit.start.y), imageSize.height))
  } else {
    if (selectionMode.value === 'point' && Math.hypot(event.clientX - edit.clientStart.x, event.clientY - edit.clientStart.y) < 5) return
    const startX = clamp(Math.round(edit.start.x), imageSize.width)
    const startY = clamp(Math.round(edit.start.y), imageSize.height)
    left = Math.min(startX, clamp(Math.round(x), imageSize.width)); top = Math.min(startY, clamp(Math.round(y), imageSize.height))
    right = Math.max(startX, clamp(Math.round(x), imageSize.width)); bottom = Math.max(startY, clamp(Math.round(y), imageSize.height))
    left = Math.min(left, imageSize.width - 1); top = Math.min(top, imageSize.height - 1)
    right = Math.max(right, left + 1); bottom = Math.max(bottom, top + 1)
  }
  selectedTarget.value = { kind: 'rect', left, top, width: right - left, height: bottom - top }
  selectedPoint.x = null; selectedPoint.y = null
  syncStageSize()
}

function handleStagePointerUp(event: PointerEvent) {
  if (!edit || edit.pointerId !== event.pointerId) return
  handleStagePointerMove(event)
  if (edit.handle === 'create' && selectionMode.value === 'point' && Math.hypot(event.clientX - edit.clientStart.x, event.clientY - edit.clientStart.y) < 5) selectPoint(event)
  suppressStageClick = true
  endSelectionEdit()
}

function changeSelectionMode(mode: 'point' | 'rect') {
  endSelectionEdit()
  selectionMode.value = mode
  if (mode === 'point') { selectedTarget.value = null; selectedPoint.x = null; selectedPoint.y = null }
}

function handleTap() {
  if (!canTap.value) return
  const target = selectedTarget.value?.kind === 'rect'
    ? selectedTarget.value
    : { kind: 'point' as const, x: Number(selectedPoint.x), y: Number(selectedPoint.y) }
  tapCountdown.value = appState.value.clickSettings.delayMaximumMs
  tapCountdownTimer = window.setInterval(() => { tapCountdown.value = Math.max(0, tapCountdown.value - 100) }, 100)
  void runOperation('tap', async () => {
    try { return await tapScreen(target) }
    finally { if (tapCountdownTimer) window.clearInterval(tapCountdownTimer); tapCountdownTimer = null; tapCountdown.value = 0 }
  }, (receipt) => {
    setStatus(
      'success',
      `已在 ${receipt.deviceSerial} 执行点击 (${receipt.finalPoint.x}, ${receipt.finalPoint.y})，等待 ${receipt.delayMs} ms，按压 ${receipt.pressDurationMs} ms。`,
    )
  })
}

function saveClickConfiguration() {
  void runOperation('activity', () => setClickSettings(appState.value.clickSettings), (state) => {
    applyState(state); setStatus('success', '随机点击配置已保存。')
  })
}

function handleCancelTap() {
  void cancelPendingClick()
  setStatus('neutral', '已请求取消待执行点击。')
}

onMounted(() => {
  void initialize()
  syncStageSize()
  if (window.ResizeObserver && screenshotStage.value) {
    stageResizeObserver = new ResizeObserver(syncStageSize)
    stageResizeObserver.observe(screenshotStage.value)
  }
})
onBeforeUnmount(() => {
  if (activityTimer) window.clearTimeout(activityTimer)
  if (tapCountdownTimer) window.clearInterval(tapCountdownTimer)
  stageResizeObserver?.disconnect()
  clearScreenshot()
  closePreviewDecoder()
})
</script>

<template>
  <main class="app-shell">
    <header class="app-header">
      <div class="brand-mark" aria-hidden="true">
        <MonitorSmartphone :size="23" :stroke-width="1.8" />
      </div>
      <div>
        <h1>OnmyojiSupportTools</h1>
        <p>Android 模拟器坐标调试工具</p>
      </div>
      <span class="version-badge">0.1.0 · 开发版</span>
    </header>

    <section class="device-toolbar" aria-label="设备连接">
      <div class="toolbar-field toolbar-field-wide">
        <label for="adb-path">ADB</label>
        <select
          id="adb-path"
          :value="appState.selectedAdb?.path ?? ''"
          :disabled="Boolean(busy)"
          @change="handleAdbChange"
        >
          <option value="" disabled>选择检测到的 ADB</option>
          <option v-for="candidate in appState.adbCandidates" :key="candidate.path" :value="candidate.path">
            {{ adbSourceLabels[candidate.source] }} · {{ candidate.version }} · {{ candidate.path }}
          </option>
        </select>
        <button class="button secondary" type="button" :disabled="Boolean(busy)" @click="chooseAdb">
          <FolderOpen :size="17" aria-hidden="true" />
          选择文件
        </button>
      </div>

      <div class="toolbar-field">
        <label for="device-select">活动设备</label>
        <select
          id="device-select"
          :value="appState.activeDeviceSerial ?? ''"
          :disabled="!appState.selectedAdb || Boolean(busy)"
          @change="handleDeviceChange"
        >
          <option value="" disabled>{{ onlineDevices.length ? '选择在线设备' : '没有在线设备' }}</option>
          <option v-for="device in appState.devices" :key="device.serial" :value="device.serial" :disabled="device.status !== 'online'">
            {{ device.model || device.serial }} · {{ deviceStatusLabels[device.status] }}
          </option>
        </select>
        <button
          class="icon-button"
          type="button"
          aria-label="刷新设备列表"
          :disabled="!appState.selectedAdb || Boolean(busy)"
          @click="handleRefreshDevices"
        >
          <RefreshCw :size="18" :class="{ spinning: busy === 'devices' }" aria-hidden="true" />
        </button>
        <button v-if="busy === 'tap'" class="button secondary tap-button" type="button" @click="handleCancelTap">取消点击（最多 {{ tapCountdown }} ms）</button>
      </div>

      <form class="toolbar-field endpoint-field" @submit.prevent="handleConnect">
        <label for="endpoint-host">手动连接</label>
        <input id="endpoint-host" v-model="endpointHost" type="text" inputmode="decimal" aria-label="设备 IP 地址" />
        <span class="endpoint-separator" aria-hidden="true">:</span>
        <input
          id="endpoint-port"
          v-model="endpointPort"
          class="port-input"
          type="number"
          min="1"
          max="65535"
          placeholder="端口"
          aria-label="设备端口"
        />
        <button class="button secondary" type="submit" :disabled="!appState.selectedAdb || Boolean(busy)">
          <PlugZap :size="17" aria-hidden="true" />
          连接
        </button>
      </form>
    </section>

    <section class="workspace" :aria-busy="Boolean(busy)">
      <article class="panel screenshot-panel">
        <div class="panel-header">
          <div>
            <h2>设备画面</h2>
            <p>{{ appState.activeDeviceSerial || '选择设备后获取截图' }}</p>
          </div>
          <div class="frame-actions">
            <button
              data-testid="preview-button"
              class="button secondary"
              type="button"
              :disabled="!appState.activeDeviceSerial || Boolean(busy)"
              @click="handlePreviewToggle"
            >
              <Square v-if="appState.previewDeviceSerial" :size="16" aria-hidden="true" />
              <Video v-else :size="18" aria-hidden="true" />
              {{ busy === 'preview' ? '处理中…' : appState.previewDeviceSerial ? '停止预览' : '实时预览（原型）' }}
            </button>
            <button
              data-testid="capture-button"
              class="button primary"
              type="button"
              :disabled="!canCapture"
              @click="handleCapture"
            >
              <Camera :size="18" aria-hidden="true" />
              {{ busy === 'capture' ? '正在截图…' : '刷新截图' }}
            </button>
          </div>
        </div>

        <div class="selection-tools" aria-label="选区编辑模式">
          <button class="button secondary" type="button" :aria-pressed="selectionMode === 'point'" :disabled="!hasVisualFrame || Boolean(busy)" @click="changeSelectionMode('point')">选择单点</button>
          <button class="button secondary" type="button" :aria-pressed="selectionMode === 'rect'" :disabled="!hasVisualFrame || Boolean(busy)" @click="changeSelectionMode('rect')">创建区域</button>
          <span>拖动画面框选；拖动区域或中心十字移动，边角手柄缩放。小选区手柄在外围。</span>
        </div>
        <div
          ref="screenshotStage"
          data-testid="screenshot-stage"
          class="screenshot-stage"
          :class="{ interactive: hasVisualFrame && !busy }"
          @click="handleStageClick"
          @pointerdown="handleStagePointerDown"
          @pointermove="handleStagePointerMove"
          @pointerup="handleStagePointerUp"
          @pointercancel="endSelectionEdit"
          @lostpointercapture="endSelectionEdit"
        >
          <img
            v-if="screenshotUrl"
            :src="screenshotUrl"
            :alt="`${appState.activeDeviceSerial || '活动设备'} 的当前截图`"
            draggable="false"
            @load="handleImageLoad"
          />
          <canvas
            ref="previewCanvas"
            class="preview-canvas"
            :class="{ visible: appState.previewDeviceSerial }"
            width="1280"
            height="720"
            aria-label="活动设备实时预览"
          />
          <div v-if="!screenshotUrl && !appState.previewDeviceSerial" class="empty-state">
            <MonitorSmartphone :size="42" :stroke-width="1.4" aria-hidden="true" />
            <strong>尚无设备截图</strong>
            <span>选择在线设备，然后点击“刷新截图”。</span>
          </div>
          <span class="coordinate-marker" :style="markerStyle" aria-hidden="true" />
          <span v-if="selectedTarget?.kind === 'rect'" class="selection-rect" :style="selectionStyle" aria-hidden="true">
            <span class="selection-label">{{ selectedTarget.width }} × {{ selectedTarget.height }} px</span>
          </span>
          <span v-if="selectedTarget?.kind === 'rect'" class="selection-controls" :style="selectionControlsStyle" aria-hidden="true">
            <span v-for="handle in resizeHandles" :key="handle" class="selection-handle" :class="`handle-${handle}`" :data-resize-handle="handle" />
            <span class="selection-move" data-selection-move title="移动区域"><Move :size="14" /></span>
          </span>
        </div>
      </article>

      <aside class="panel inspector-panel" aria-labelledby="coordinate-title">
        <div class="inspector-heading">
          <div class="icon-tile" aria-hidden="true">
            <MousePointerClick :size="19" />
          </div>
          <div>
            <h2 id="coordinate-title">坐标检查器</h2>
            <p>选中后再明确执行点击</p>
          </div>
        </div>

        <dl class="frame-meta">
          <div>
            <dt>截图尺寸</dt>
            <dd>{{ imageSize.width ? `${imageSize.width} × ${imageSize.height}` : '—' }}</dd>
          </div>
          <div>
            <dt>设备状态</dt>
            <dd :class="{ online: appState.activeDeviceSerial }">
              {{ appState.activeDeviceSerial ? '在线' : '未选择' }}
            </dd>
          </div>
        </dl>

        <div class="coordinate-fields">
          <div>
            <label for="coordinate-x">X 坐标</label>
            <input
              id="coordinate-x"
              v-model.number="selectedPoint.x"
              type="number"
              min="0"
              :max="Math.max(0, imageSize.width - 1)"
              :disabled="!imageSize.width || Boolean(busy) || selectedTarget?.kind === 'rect'"
            />
          </div>
          <div>
            <label for="coordinate-y">Y 坐标</label>
            <input
              id="coordinate-y"
              v-model.number="selectedPoint.y"
              type="number"
              min="0"
              :max="Math.max(0, imageSize.height - 1)"
              :disabled="!imageSize.height || Boolean(busy) || selectedTarget?.kind === 'rect'"
            />
          </div>
        </div>
        <div class="target-editor">
          <p v-if="selectedTarget?.kind === 'rect'" data-testid="selection-size" class="safety-note">点击区域：{{ selectedTarget.width }} × {{ selectedTarget.height }} px</p>
          <button v-if="hasVisualFrame && selectedTarget?.kind !== 'rect'" class="button secondary tap-button" type="button" :disabled="Boolean(busy)" @click="createRectFromPoint">从当前点创建可编辑区域</button>
          <div v-if="selectedTarget?.kind === 'rect'" class="click-settings" aria-label="点击区域像素边界">
            <label>Left<input v-model.number="selectedTarget.left" type="number" min="0" @input="normalizeKeyboardRect" /></label>
            <label>Top<input v-model.number="selectedTarget.top" type="number" min="0" @input="normalizeKeyboardRect" /></label>
            <label>Width<input v-model.number="selectedTarget.width" type="number" min="1" @input="normalizeKeyboardRect" /></label>
            <label>Height<input v-model.number="selectedTarget.height" type="number" min="1" @input="normalizeKeyboardRect" /></label>
          </div>
        </div>
        <div class="click-settings">
          <label>延时下限<input v-model.number="appState.clickSettings.delayMinimumMs" type="number" min="0" /></label>
          <label>延时上限<input v-model.number="appState.clickSettings.delayMaximumMs" type="number" min="0" /></label>
          <label>点偏移半径<input v-model.number="appState.clickSettings.pointRadius" type="number" min="1" /></label>
          <label>按压下限<input v-model.number="appState.clickSettings.pressMinimumMs" type="number" min="1" /></label>
          <label>按压上限<input v-model.number="appState.clickSettings.pressMaximumMs" type="number" min="1" /></label>
        </div>
        <button class="button secondary tap-button" type="button" :disabled="Boolean(busy)" @click="saveClickConfiguration">保存随机点击配置</button>

        <button
          data-testid="tap-button"
          class="button primary tap-button"
          type="button"
          :disabled="!canTap"
          @click="handleTap"
        >
          <MousePointerClick :size="18" aria-hidden="true" />
          {{ busy === 'tap' ? '正在执行…' : '执行点击' }}
        </button>

        <p class="safety-note">
          点击截图只会选择坐标；只有按下此按钮后才会操作设备。
        </p>
      </aside>
    </section>

    <section class="panel activity-panel" aria-labelledby="activity-title">
      <div class="panel-header">
        <div><h2 id="activity-title">结界突破校准与自动挑战</h2><p>按成功结算计数，最多 30 次；失败不会增加完成次数。</p></div>
        <span class="version-badge">配置 v1</span>
      </div>
      <div class="activity-grid">
        <div class="activity-card">
          <h3>本机校准</h3>
          <label for="activity-name">配置名称</label>
          <input id="activity-name" v-model="activityName" type="text" />
          <button class="button secondary" type="button" :disabled="Boolean(busy)" @click="newCalibration">新建配置</button>
          <label for="calibration-state">当前页面</label>
          <select id="calibration-state" v-model="selectedCalibrationState">
            <option v-for="state in calibrationStates" :key="state" :value="state">{{ pageLabels[state] }}</option>
          </select>
          <p class="safety-note">先刷新截图并框选稳定区域。特征只有 4×4 色彩统计，无法还原画面。</p>
          <div class="activity-actions">
            <button class="button secondary" type="button" :disabled="!selectedRect() || Boolean(busy)" @click="addCalibrationFeature">添加识别区域</button>
            <button v-if="selectedCalibrationState !== 'battling' && selectedCalibrationState !== 'challenge'" class="button secondary" type="button" :disabled="!selectedRect() || Boolean(busy)" @click="setCalibrationAction">设为动作区域</button>
            <button v-if="selectedCalibrationState === 'challenge' && draftConfig?.kind === 'realmRaid'" class="button secondary" type="button" :disabled="!selectedRect() || Boolean(busy)" @click="addRealmRaidOpponentAction">添加对手区域</button>
            <button v-if="selectedCalibrationState === 'challenge' && draftConfig?.kind === 'realmRaid'" class="button secondary" type="button" :disabled="!selectedRect() || !draftConfig.realmRaid?.opponents.length || Boolean(busy)" @click="addOpponentAvailabilityFeature">添加当前对手可挑战特征</button>
            <button v-if="selectedCalibrationState === 'challenge' && draftConfig?.kind === 'realmRaid'" class="button secondary" type="button" :disabled="!selectedRect() || Boolean(busy)" @click="setRealmRaidRefreshAction">设为刷新区域</button>
            <button v-if="selectedCalibrationState === 'challenge' && draftConfig?.kind === 'realmRaid'" class="button secondary" type="button" :disabled="!selectedRect() || Boolean(busy)" @click="addRefreshAvailabilityFeature">添加刷新可用特征</button>
            <button v-if="selectedCalibrationState === 'challenge' && draftConfig?.kind === 'realmRaid'" class="button secondary" type="button" :disabled="!selectedRect() || Boolean(busy)" @click="addProgressRewardAction">添加 3/6/9 奖励区域</button>
            <button v-if="selectedCalibrationState === 'challenge' && draftConfig?.kind === 'realmRaid'" class="button secondary" type="button" :disabled="!selectedRect() || !draftConfig.realmRaid?.progressRewards.length || Boolean(busy)" @click="addProgressRewardFeature">添加当前奖励可领取特征</button>
            <button v-if="selectedCalibrationState === 'opponent' && draftConfig?.kind === 'realmRaid'" class="button secondary" type="button" :disabled="!selectedRect() || Boolean(busy)" @click="addAttackRequirement">添加进攻门禁特征</button>
            <button class="button primary" type="button" :disabled="!draftConfig || Boolean(busy)" @click="saveCalibration">保存配置</button>
          </div>
          <div class="click-settings">
            <label>动作延时下限<input v-model.number="overrideDelayMinimum" type="number" min="0" placeholder="继承全局" /></label>
            <label>动作延时上限<input v-model.number="overrideDelayMaximum" type="number" min="0" placeholder="继承全局" /></label>
            <label>动作按压下限<input v-model.number="overridePressMinimum" type="number" min="1" placeholder="继承全局" /></label>
            <label>动作按压上限<input v-model.number="overridePressMaximum" type="number" min="1" placeholder="继承全局" /></label>
          </div>
          <button class="button secondary" type="button" :disabled="!draftConfig || selectedCalibrationState === 'battling'" @click="applyActionTimingOverride">应用逐动作时序覆盖</button>
          <label for="popup-name">已知弹窗（可选）</label>
          <input id="popup-name" v-model="popupName" type="text" />
          <div class="activity-actions">
            <button class="button secondary" type="button" :disabled="!selectedRect() || Boolean(busy)" @click="addPopupFeature">添加弹窗识别区域</button>
            <button class="button secondary" type="button" :disabled="!selectedRect() || Boolean(busy)" @click="setPopupCloseAction">设为弹窗关闭区域</button>
            <button class="button secondary" type="button" :disabled="!draftConfig || Boolean(busy)" @click="applyPopupTimingOverride">应用弹窗时序覆盖</button>
            <button v-if="draftConfig?.kind === 'realmRaid'" class="button secondary" type="button" :disabled="!selectedRect() || Boolean(busy)" @click="addPauseCondition">添加同名安全暂停条件</button>
          </div>
          <div v-if="draftConfig" class="click-settings">
            <label>全局延时下限<input v-model.number="draftConfig.clickDelay.minimumMs" type="number" min="0" /></label>
            <label>全局延时上限<input v-model.number="draftConfig.clickDelay.maximumMs" type="number" min="0" /></label>
            <label>全局按压下限<input v-model.number="draftConfig.pressDuration.minimumMs" type="number" min="1" /></label>
            <label>全局按压上限<input v-model.number="draftConfig.pressDuration.maximumMs" type="number" min="1" /></label>
          </div>
          <label v-if="draftConfig?.kind === 'realmRaid' && draftConfig.realmRaid" for="failure-limit">连续失败暂停上限</label>
          <input v-if="draftConfig?.kind === 'realmRaid' && draftConfig.realmRaid" id="failure-limit" v-model.number="draftConfig.realmRaid.failureLimit" type="number" min="1" max="9" step="1" />
          <ul v-if="draftConfig" class="calibration-summary">
            <li v-for="profile in draftConfig.states" :key="profile.state">{{ pageLabels[profile.state] }}：{{ profile.features.length }} 个识别区域<span v-if="profile.state === 'challenge' && draftConfig.kind === 'realmRaid'"> · {{ draftConfig.realmRaid?.opponents.length || 0 }} / 9 个对手（{{ draftConfig.realmRaid?.opponents.filter(item => item.availableFeatures.length).length || 0 }} 个已设可挑战特征） · {{ draftConfig.realmRaid?.refresh.features.length ? '刷新门禁已设置' : '缺少刷新门禁' }} · {{ draftConfig.realmRaid?.progressRewards.length || 0 }} / 3 个奖励</span><span v-else-if="profile.state === 'opponent' && draftConfig.kind === 'realmRaid'"> · {{ profile.action ? '进攻区域已设置' : '缺少进攻区域' }} · {{ draftConfig.realmRaid?.attackRequirements.length || 0 }} / 2+ 项门禁</span><span v-else-if="profile.state !== 'battling'"> · {{ profile.action ? '动作已设置' : '缺少动作' }}</span></li>
          </ul>
        </div>
        <div class="activity-card">
          <h3>任务控制</h3>
          <label for="activity-config">活动配置</label>
          <select id="activity-config" v-model="selectedConfigId" :disabled="appState.activitySession.status !== 'idle'" @change="handleConfigSelection">
            <option value="" disabled>选择已保存配置</option>
            <option v-for="config in appState.activityConfigs" :key="config.id" :value="config.id">{{ config.name }}</option>
          </select>
          <div class="activity-actions">
            <button class="button secondary" type="button" :disabled="!selectedConfigId || Boolean(busy)" @click="previewCalibration">预览识别评分</button>
            <button class="button secondary" type="button" :disabled="!selectedConfigId || appState.activitySession.status !== 'idle' || Boolean(busy)" @click="deleteCalibration">删除配置</button>
          </div>
          <ul v-if="recognitionPreview" class="calibration-summary">
            <li v-for="score in recognitionPreview.scores" :key="score.state">{{ pageLabels[score.state] }}：{{ (score.score * 100).toFixed(1) }}%</li>
            <li>结果：{{ recognitionPreview.matchedState ? pageLabels[recognitionPreview.matchedState] : recognitionPreview.reason }}</li>
          </ul>
          <label for="target-runs">目标成功次数</label>
          <input id="target-runs" v-model.number="targetRuns" type="number" min="1" max="30" step="1" aria-describedby="target-runs-help target-runs-error" :aria-invalid="Boolean(targetRunsError)" />
          <p id="target-runs-help" class="safety-note">按成功结算计数；失败不计次，挑战券持有上限为 30。</p>
          <p v-if="targetRunsError" id="target-runs-error" class="field-error" role="alert">{{ targetRunsError }}</p>
          <dl class="frame-meta">
            <div><dt>任务状态</dt><dd>{{ appState.activitySession.status }}</dd></div>
            <div><dt>完成次数</dt><dd>{{ appState.activitySession.completedRuns }} / {{ appState.activitySession.targetRuns || targetRuns }}</dd></div>
            <div><dt>重试</dt><dd>{{ appState.activitySession.retryCount }} / 3</dd></div>
            <div><dt>暂停原因</dt><dd>{{ appState.activitySession.pauseReason?.message || '—' }}</dd></div>
          </dl>
          <div class="activity-actions">
            <button data-testid="activity-start" class="button primary" type="button" :disabled="appState.activitySession.status !== 'idle' || Boolean(busy)" @click="handleStartActivity">开始</button>
            <button class="button secondary" type="button" :disabled="['idle','paused','completed','failed'].includes(appState.activitySession.status) || Boolean(busy)" @click="handlePauseActivity">暂停</button>
            <button v-if="appState.activitySession.status === 'paused'" class="button secondary" type="button" :disabled="Boolean(busy)" @click="handleResumePreview">重新识别</button>
            <button v-if="resumePreview?.matchedState" class="button primary" type="button" :disabled="Boolean(busy)" @click="handleResumeConfirm">确认恢复</button>
            <button class="button secondary" type="button" :disabled="appState.activitySession.status === 'idle' || Boolean(busy)" @click="handleStopActivity">停止</button>
          </div>
          <p class="safety-note">本工具不承诺规避或绕过游戏检测、风控或服务条款；请遵守适用规则。</p>
        </div>
      </div>
    </section>

    <footer class="status-bar" :class="`status-${status.tone}`" aria-live="polite">
      <CheckCircle2 v-if="status.tone === 'success'" :size="17" aria-hidden="true" />
      <CircleAlert v-else-if="status.tone === 'error'" :size="17" aria-hidden="true" />
      <RefreshCw v-else-if="busy" :size="17" class="spinning" aria-hidden="true" />
      <span>{{ status.message }}</span>
      <span v-if="status.recovery" class="recovery">{{ status.recovery }}</span>
    </footer>
  </main>
</template>
