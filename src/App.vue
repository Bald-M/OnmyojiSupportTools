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
  tapScreen,
} from './lib/device'
import type { AdbSource, AppError, AppState, DeviceStatus } from './types/device'

type Operation = 'initialize' | 'adb' | 'devices' | 'connect' | 'capture' | 'tap'
type StatusTone = 'neutral' | 'success' | 'error'

const emptyState: AppState = {
  adbCandidates: [],
  selectedAdb: null,
  devices: [],
  activeDeviceSerial: null,
  lastFrame: null,
  lastEndpoint: null,
}

const appState = ref<AppState>(emptyState)
const busy = ref<Operation | null>('initialize')
const screenshotUrl = shallowRef<string | null>(null)
const screenshotStage = ref<HTMLElement | null>(null)
const imageSize = reactive({ width: 0, height: 0 })
const stageSize = reactive({ width: 0, height: 0 })
const selectedPoint = reactive<{ x: number | null; y: number | null }>({
  x: null,
  y: null,
})
const endpointHost = ref('127.0.0.1')
const endpointPort = ref('')
const status = reactive<{ tone: StatusTone; message: string; recovery: string | null }>({
  tone: 'neutral',
  message: '正在初始化设备环境…',
  recovery: null,
})
let stageResizeObserver: ResizeObserver | null = null
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
  () => Boolean(appState.value.selectedAdb && appState.value.activeDeviceSerial) && !busy.value,
)
const hasSelectedPoint = computed(
  () =>
    Boolean(screenshotUrl.value && appState.value.activeDeviceSerial) &&
    imageSize.width > 0 &&
    Number.isInteger(selectedPoint.x) &&
    Number.isInteger(selectedPoint.y) &&
    Number(selectedPoint.x) >= 0 &&
    Number(selectedPoint.y) >= 0 &&
    Number(selectedPoint.x) < imageSize.width &&
    Number(selectedPoint.y) < imageSize.height,
)
const canTap = computed(() => hasSelectedPoint.value && !busy.value)
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
}

function applyState(nextState: AppState) {
  const deviceChanged = appState.value.activeDeviceSerial !== nextState.activeDeviceSerial
  appState.value = nextState
  if (deviceChanged) clearScreenshot()
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
  if (!screenshotUrl.value || imageSize.width <= 0 || !screenshotStage.value) return
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
  setStatus('neutral', `已选择坐标 (${point.x}, ${point.y})，等待执行。`)
}

function handleTap() {
  if (!canTap.value) return
  const point = { x: Number(selectedPoint.x), y: Number(selectedPoint.y) }
  void runOperation('tap', () => tapScreen(point), (receipt) => {
    setStatus(
      'success',
      `已在 ${receipt.deviceSerial} 执行点击 (${receipt.point.x}, ${receipt.point.y})。`,
    )
  })
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
  stageResizeObserver?.disconnect()
  clearScreenshot()
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

        <div
          ref="screenshotStage"
          data-testid="screenshot-stage"
          class="screenshot-stage"
          :class="{ interactive: screenshotUrl && !busy }"
          @click="handleStageClick"
        >
          <img
            v-if="screenshotUrl"
            :src="screenshotUrl"
            :alt="`${appState.activeDeviceSerial || '活动设备'} 的当前截图`"
            draggable="false"
            @load="handleImageLoad"
          />
          <div v-else class="empty-state">
            <MonitorSmartphone :size="42" :stroke-width="1.4" aria-hidden="true" />
            <strong>尚无设备截图</strong>
            <span>选择在线设备，然后点击“刷新截图”。</span>
          </div>
          <span class="coordinate-marker" :style="markerStyle" aria-hidden="true" />
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

    <footer class="status-bar" :class="`status-${status.tone}`" aria-live="polite">
      <CheckCircle2 v-if="status.tone === 'success'" :size="17" aria-hidden="true" />
      <CircleAlert v-else-if="status.tone === 'error'" :size="17" aria-hidden="true" />
      <RefreshCw v-else-if="busy" :size="17" class="spinning" aria-hidden="true" />
      <span>{{ status.message }}</span>
      <span v-if="status.recovery" class="recovery">{{ status.recovery }}</span>
    </footer>
  </main>
</template>
