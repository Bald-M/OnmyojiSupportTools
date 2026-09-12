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
} from '@lucide/vue'
import { open } from '@tauri-apps/plugin-dialog'
import { computed, onBeforeUnmount, onMounted, reactive, ref, shallowRef } from 'vue'

import { calculateContainedImageRect, mapClientDragToImage, mapClientPointToImage } from './lib/coordinates'
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
  activitySession: { configId: '', status: 'idle', currentState: null, targetRuns: 0, completedRuns: 0, retryCount: 0, pauseReason: null, lastSafeAction: null, lastEvent: null },
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
let dragStart: { x: number; y: number } | null = null
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
const activityName = ref('活动配置')
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
const pageLabels: Record<PageState, string> = { activityEntry: '活动入口', stageEntry: '关卡入口', challenge: '首次挑战页', battling: '战斗中', reward: '奖励页', returnChallenge: '返回挑战页' }
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
    version: 1, id: window.crypto.randomUUID(), name: activityName.value,
    frame: { width: imageSize.width, height: imageSize.height, orientation: imageSize.width >= imageSize.height ? 'landscape' : 'portrait' },
    states: (Object.keys(pageLabels) as PageState[]).map((state) => ({ state, features: [], action: null, clickDelay: null, pressDuration: null })),
    knownPopups: [], matching: { threshold: 0.9, minimumMargin: 0.08 },
    clickDelay: { minimumMs: 300, maximumMs: 900 }, pressDuration: { minimumMs: 45, maximumMs: 120 },
  }
  return draftConfig.value
}

function selectedRect() {
  return selectedTarget.value?.kind === 'rect' ? { left: selectedTarget.value.left, top: selectedTarget.value.top, width: selectedTarget.value.width, height: selectedTarget.value.height } : null
}

function createRectFromPoint() {
  if (!imageSize.width || !imageSize.height) return
  const x = Number.isInteger(selectedPoint.x) ? Number(selectedPoint.x) : 0
  const y = Number.isInteger(selectedPoint.y) ? Number(selectedPoint.y) : 0
  selectedTarget.value = { kind: 'rect', left: x, top: y, width: Math.min(40, imageSize.width - x), height: Math.min(40, imageSize.height - y) }
  selectedPoint.x = null; selectedPoint.y = null
}

function normalizeKeyboardRect() {
  const target = selectedTarget.value
  if (target?.kind !== 'rect') return
  target.left = Math.max(0, Math.min(Math.trunc(target.left), imageSize.width - 1))
  target.top = Math.max(0, Math.min(Math.trunc(target.top), imageSize.height - 1))
  target.width = Math.max(1, Math.min(Math.trunc(target.width), imageSize.width - target.left))
  target.height = Math.max(1, Math.min(Math.trunc(target.height), imageSize.height - target.top))
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
  activityName.value = '活动配置'
  recognitionPreview.value = null
  if (imageSize.width && imageSize.height) ensureDraft()
  setStatus('neutral', '已开始新配置；请按六个页面状态依次校准。')
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
    imageSize.width = 0
    imageSize.height = 0
    selectedPoint.x = null
    selectedPoint.y = null
    selectedTarget.value = null
    previewFailurePending = false
    setStatus('error', message, '实时预览已停止，请继续使用“刷新截图”。')
  }
}

function applyState(nextState: AppState) {
  const deviceChanged = appState.value.activeDeviceSerial !== nextState.activeDeviceSerial
  appState.value = nextState
  if (deviceChanged) {
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

function handlePreviewToggle() {
  if (appState.value.previewDeviceSerial) {
    void runOperation('preview', stopPreview, (state) => {
      applyState(state)
      closePreviewDecoder()
      imageSize.width = 0
      imageSize.height = 0
      selectedPoint.x = null
      selectedPoint.y = null
      selectedTarget.value = null
      setStatus('neutral', '实时预览已停止，可继续使用静态截图。')
    })
    return
  }
  clearScreenshot()
  imageSize.width = 1280
  imageSize.height = 720
  void runOperation('preview', () => startPreview(handlePreviewChunk, () => {
    void recoverFromPreviewFailure('实时预览流已结束或超过 8 秒没有画面数据。')
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

function handleStageClick(event: MouseEvent) {
  if (!hasVisualFrame.value || imageSize.width <= 0 || !screenshotStage.value) return
  const rect = screenshotStage.value.getBoundingClientRect()
  syncStageSize()
  const point = mapClientPointToImage(
    { x: event.clientX, y: event.clientY },
    { left: rect.left, top: rect.top, width: rect.width, height: rect.height },
    imageSize,
  )
  if (!point) return
  selectedPoint.x = point.x
  selectedPoint.y = point.y
  selectedTarget.value = { kind: 'point', ...point }
  setStatus('neutral', `已选择坐标 (${point.x}, ${point.y})，等待执行。`)
}

function handleStageMouseDown(event: MouseEvent) { dragStart = { x: event.clientX, y: event.clientY } }

function handleStageMouseUp(event: MouseEvent) {
  if (!dragStart || !screenshotStage.value || !hasVisualFrame.value) return
  const rect = screenshotStage.value.getBoundingClientRect()
  const target = mapClientDragToImage(dragStart, { x: event.clientX, y: event.clientY }, rect, imageSize, 5)
  dragStart = null
  if (!target || target.kind !== 'rect') return
  selectedTarget.value = target
  selectedPoint.x = null
  selectedPoint.y = null
  setStatus('neutral', `已选择区域 ${target.width} × ${target.height}，等待执行。`)
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

        <div
          ref="screenshotStage"
          data-testid="screenshot-stage"
          class="screenshot-stage"
          :class="{ interactive: hasVisualFrame && !busy }"
          @click="handleStageClick"
          @mousedown="handleStageMouseDown"
          @mouseup="handleStageMouseUp"
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
          <span class="selection-rect" :style="selectionStyle" aria-hidden="true" />
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
              :disabled="!imageSize.width || Boolean(busy)"
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
              :disabled="!imageSize.height || Boolean(busy)"
            />
          </div>
        </div>
        <p v-if="selectedTarget?.kind === 'rect'" data-testid="selection-size" class="safety-note">点击区域：{{ selectedTarget.width }} × {{ selectedTarget.height }} px</p>
        <button v-if="hasVisualFrame && selectedTarget?.kind !== 'rect'" class="button secondary tap-button" type="button" :disabled="Boolean(busy)" @click="createRectFromPoint">从当前点创建可编辑区域</button>
        <div v-if="selectedTarget?.kind === 'rect'" class="click-settings" aria-label="点击区域像素边界">
          <label>Left<input v-model.number="selectedTarget.left" type="number" min="0" @change="normalizeKeyboardRect" /></label>
          <label>Top<input v-model.number="selectedTarget.top" type="number" min="0" @change="normalizeKeyboardRect" /></label>
          <label>Width<input v-model.number="selectedTarget.width" type="number" min="1" @change="normalizeKeyboardRect" /></label>
          <label>Height<input v-model.number="selectedTarget.height" type="number" min="1" @change="normalizeKeyboardRect" /></label>
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
        <div><h2 id="activity-title">活动校准与自动挑战</h2><p>仅操作当前活动设备；未知或歧义页面会安全暂停。</p></div>
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
            <option v-for="(label, state) in pageLabels" :key="state" :value="state">{{ label }}</option>
          </select>
          <p class="safety-note">先刷新截图并框选稳定区域。特征只有 4×4 色彩统计，无法还原画面。</p>
          <div class="activity-actions">
            <button class="button secondary" type="button" :disabled="!selectedRect() || Boolean(busy)" @click="addCalibrationFeature">添加识别区域</button>
            <button v-if="selectedCalibrationState !== 'battling'" class="button secondary" type="button" :disabled="!selectedRect() || Boolean(busy)" @click="setCalibrationAction">设为动作区域</button>
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
          </div>
          <div v-if="draftConfig" class="click-settings">
            <label>全局延时下限<input v-model.number="draftConfig.clickDelay.minimumMs" type="number" min="0" /></label>
            <label>全局延时上限<input v-model.number="draftConfig.clickDelay.maximumMs" type="number" min="0" /></label>
            <label>全局按压下限<input v-model.number="draftConfig.pressDuration.minimumMs" type="number" min="1" /></label>
            <label>全局按压上限<input v-model.number="draftConfig.pressDuration.maximumMs" type="number" min="1" /></label>
          </div>
          <ul v-if="draftConfig" class="calibration-summary">
            <li v-for="profile in draftConfig.states" :key="profile.state">{{ pageLabels[profile.state] }}：{{ profile.features.length }} 个识别区域<span v-if="profile.state !== 'battling'"> · {{ profile.action ? '动作已设置' : '缺少动作' }}</span></li>
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
          <input id="target-runs" v-model.number="targetRuns" type="number" min="1" />
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
