import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import App from './App.vue'
import type { ActivityConfig, AppState, PageState } from './types/device'

const device = vi.hoisted(() => ({
  getAppAppState: vi.fn(),
  setAdbPath: vi.fn(),
  refreshDevices: vi.fn(),
  connectDevice: vi.fn(),
  selectDevice: vi.fn(),
  captureScreen: vi.fn(),
  tapScreen: vi.fn(),
  startPreview: vi.fn(),
  stopPreview: vi.fn(),
  setClickSettings: vi.fn(), cancelPendingClick: vi.fn(), saveActivityConfig: vi.fn(), deleteActivityConfig: vi.fn(), calibrateActivityFeature: vi.fn(), previewActivityRecognition: vi.fn(), startActivity: vi.fn(), pauseActivity: vi.fn(), resumeActivity: vi.fn(), stopActivity: vi.fn(), advanceActivity: vi.fn(),
}))

vi.mock('./lib/device', () => device)
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }))

const readyState: AppState = {
  adbCandidates: [{ path: 'C:\\adb.exe', source: 'manual', version: '35.0.2' }],
  selectedAdb: { path: 'C:\\adb.exe', source: 'manual', version: '35.0.2' },
  devices: [
    {
      serial: '127.0.0.1:16384',
      model: 'MuMu 12',
      status: 'online',
      transport: '1',
    },
  ],
  activeDeviceSerial: '127.0.0.1:16384',
  lastFrame: null,
  lastEndpoint: null,
  previewDeviceSerial: null,
  clickSettings: { delayMinimumMs: 300, delayMaximumMs: 900, pointRadius: 6, pressMinimumMs: 45, pressMaximumMs: 120 },
  activityConfigs: [],
  activitySession: { configId: '', status: 'idle', currentState: null, targetRuns: 0, completedRuns: 0, retryCount: 0, pauseReason: null, lastSafeAction: null, lastEvent: null },
}

const pageStates: PageState[] = ['activityEntry', 'stageEntry', 'challenge', 'battling', 'reward', 'returnChallenge']
const activityConfig: ActivityConfig = {
  version: 1, id: 'activity-1', name: '测试活动', frame: { width: 1280, height: 720, orientation: 'landscape' },
  states: pageStates.map((state) => ({ state, features: [], action: null, clickDelay: null, pressDuration: null })),
  knownPopups: [], matching: { threshold: 0.9, minimumMargin: 0.08 }, clickDelay: { minimumMs: 0, maximumMs: 0 }, pressDuration: { minimumMs: 1, maximumMs: 1 },
}

describe('desktop device workflow', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    device.getAppAppState.mockResolvedValue(readyState)
    device.captureScreen.mockResolvedValue(new Uint8Array([137, 80, 78, 71]))
    device.tapScreen.mockResolvedValue({
      deviceSerial: '127.0.0.1:16384',
      target: { kind: 'point', x: 640, y: 360 }, delayMs: 300, finalPoint: { x: 641, y: 360 }, pressDurationMs: 60,
      completedAt: 1,
    })
    device.startPreview.mockResolvedValue({ ...readyState, previewDeviceSerial: readyState.activeDeviceSerial })
    device.stopPreview.mockResolvedValue(readyState)
    device.startActivity.mockResolvedValue({ ...readyState.activitySession, configId: 'activity-1', status: 'completed', targetRuns: 1, completedRuns: 1 })
    device.pauseActivity.mockResolvedValue({ ...readyState.activitySession, configId: 'activity-1', status: 'paused', pauseReason: { code: 'user', message: '用户暂停' } })
    device.resumeActivity.mockResolvedValue({ ...readyState.activitySession, configId: 'activity-1', status: 'completed', targetRuns: 1, completedRuns: 1 })
    device.previewActivityRecognition.mockResolvedValue({ matchedState: 'challenge', scores: [{ state: 'challenge', score: 0.99 }], reason: null })
    device.saveActivityConfig.mockResolvedValue([activityConfig])
    device.deleteActivityConfig.mockResolvedValue([])
    device.pauseActivity.mockResolvedValue({ ...readyState.activitySession, configId: 'activity-1', status: 'paused', targetRuns: 1, pauseReason: { code: 'user', message: '用户暂停' } })
    device.stopActivity.mockResolvedValue(readyState.activitySession)
    device.advanceActivity.mockResolvedValue({ ...readyState.activitySession, configId: 'activity-1', status: 'completed', targetRuns: 1, completedRuns: 1 })
  })

  it('requires an explicit action after choosing a screenshot coordinate', async () => {
    const wrapper = mount(App)
    await flushPromises()

    await wrapper.get('[data-testid="capture-button"]').trigger('click')
    await flushPromises()

    const image = wrapper.get('img').element as HTMLImageElement
    Object.defineProperties(image, {
      naturalWidth: { configurable: true, value: 1280 },
      naturalHeight: { configurable: true, value: 720 },
    })
    await wrapper.get('img').trigger('load')

    const stage = wrapper.get('[data-testid="screenshot-stage"]')
    vi.spyOn(stage.element, 'getBoundingClientRect').mockReturnValue({
      left: 0,
      top: 0,
      width: 1000,
      height: 700,
      right: 1000,
      bottom: 700,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    })
    await stage.trigger('click', { clientX: 500, clientY: 350 })

    expect((wrapper.get('#coordinate-x').element as HTMLInputElement).value).toBe('640')
    expect((wrapper.get('#coordinate-y').element as HTMLInputElement).value).toBe('360')
    expect(device.tapScreen).not.toHaveBeenCalled()

    await wrapper.get('[data-testid="tap-button"]').trigger('click')
    await flushPromises()
    expect(device.tapScreen).toHaveBeenCalledWith({ kind: 'point', x: 640, y: 360 })
  })

  it('accepts keyboard coordinates and releases replaced screenshot URLs', async () => {
    const wrapper = mount(App)
    await flushPromises()

    await wrapper.get('[data-testid="capture-button"]').trigger('click')
    await flushPromises()
    const image = wrapper.get('img').element as HTMLImageElement
    Object.defineProperties(image, {
      naturalWidth: { configurable: true, value: 1280 },
      naturalHeight: { configurable: true, value: 720 },
    })
    await wrapper.get('img').trigger('load')
    await wrapper.get('#coordinate-x').setValue('32')
    await wrapper.get('#coordinate-y').setValue('48')
    await wrapper.get('[data-testid="tap-button"]').trigger('click')
    await flushPromises()

    expect(device.tapScreen).toHaveBeenCalledWith({ kind: 'point', x: 32, y: 48 })

    await wrapper.get('[data-testid="capture-button"]').trigger('click')
    await flushPromises()
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:test-screenshot')

    wrapper.unmount()
    expect(URL.revokeObjectURL).toHaveBeenCalledTimes(2)
  })

  it('normalizes a dragged region and submits it as the click target', async () => {
    const wrapper = mount(App)
    await flushPromises()
    await wrapper.get('[data-testid="capture-button"]').trigger('click')
    await flushPromises()
    const image = wrapper.get('img').element as HTMLImageElement
    Object.defineProperties(image, { naturalWidth: { configurable: true, value: 1000 }, naturalHeight: { configurable: true, value: 500 } })
    await wrapper.get('img').trigger('load')
    const stage = wrapper.get('[data-testid="screenshot-stage"]')
    vi.spyOn(stage.element, 'getBoundingClientRect').mockReturnValue({ left: 0, top: 0, width: 1000, height: 750, right: 1000, bottom: 750, x: 0, y: 0, toJSON: () => ({}) })
    await stage.trigger('mousedown', { clientX: 750, clientY: 500 })
    await stage.trigger('mouseup', { clientX: 250, clientY: 250 })

    expect(wrapper.get('[data-testid="selection-size"]').text()).toContain('500 × 250')
    await wrapper.get('[data-testid="tap-button"]').trigger('click')
    await flushPromises()
    expect(device.tapScreen).toHaveBeenCalledWith({ kind: 'rect', left: 250, top: 125, width: 500, height: 250 })
  })

  it('shows a recoverable backend error without hiding the controls', async () => {
    device.getAppAppState.mockRejectedValueOnce({
      code: 'ADB_NOT_FOUND',
      message: '未检测到 ADB',
      recovery: '请选择 adb.exe',
    })

    const wrapper = mount(App)
    await flushPromises()

    expect(wrapper.get('.status-bar').text()).toContain('未检测到 ADB')
    expect(wrapper.get('.status-bar').text()).toContain('请选择 adb.exe')
    expect(wrapper.find('main').exists()).toBe(true)
  })

  it('shows unavailable devices but keeps them and capture disabled', async () => {
    device.getAppAppState.mockResolvedValueOnce({
      ...readyState,
      activeDeviceSerial: null,
      devices: [
        { serial: 'offline-device', model: null, status: 'offline', transport: '2' },
        {
          serial: 'unauthorized-device',
          model: null,
          status: 'unauthorized',
          transport: '3',
        },
      ],
    } satisfies AppState)

    const wrapper = mount(App)
    await flushPromises()

    const options = wrapper.findAll('#device-select option')
    expect(options[1]?.text()).toContain('离线')
    expect(options[1]?.attributes('disabled')).toBeDefined()
    expect(options[2]?.text()).toContain('未授权')
    expect(options[2]?.attributes('disabled')).toBeDefined()
    expect(wrapper.get('[data-testid="capture-button"]').attributes('disabled')).toBeDefined()
  })

  it('starts and stops preview explicitly without tapping the device', async () => {
    const wrapper = mount(App)
    await flushPromises()

    await wrapper.get('[data-testid="preview-button"]').trigger('click')
    await flushPromises()
    expect(device.startPreview).toHaveBeenCalledOnce()
    expect(device.tapScreen).not.toHaveBeenCalled()

    const stage = wrapper.get('[data-testid="screenshot-stage"]')
    vi.spyOn(stage.element, 'getBoundingClientRect').mockReturnValue({
      left: 0,
      top: 0,
      width: 1280,
      height: 720,
      right: 1280,
      bottom: 720,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    })
    await stage.trigger('click', { clientX: 320, clientY: 180 })
    expect((wrapper.get('#coordinate-x').element as HTMLInputElement).value).toBe('320')
    expect((wrapper.get('#coordinate-y').element as HTMLInputElement).value).toBe('180')
    expect(device.tapScreen).not.toHaveBeenCalled()

    await wrapper.get('[data-testid="preview-button"]').trigger('click')
    await flushPromises()
    expect(device.stopPreview).toHaveBeenCalledOnce()
  })

  it('returns to static capture when the preview stream ends', async () => {
    let endPreview = () => {}
    device.startPreview.mockImplementationOnce(
      (_onChunk: (chunk: Uint8Array) => void, onEnded: () => void) => {
        endPreview = onEnded
        return Promise.resolve({ ...readyState, previewDeviceSerial: readyState.activeDeviceSerial })
      },
    )
    const wrapper = mount(App)
    await flushPromises()
    await wrapper.get('[data-testid="preview-button"]').trigger('click')
    await flushPromises()

    endPreview()
    await flushPromises()

    expect(device.stopPreview).toHaveBeenCalledOnce()
    expect(wrapper.get('[data-testid="capture-button"]').attributes('disabled')).toBeUndefined()
    expect(wrapper.get('.status-bar').text()).toContain('刷新截图')
  })

  it('starts a saved activity and requires recognition before resume confirmation', async () => {
    device.getAppAppState.mockResolvedValueOnce({ ...readyState, activityConfigs: [activityConfig] })
    const wrapper = mount(App)
    await flushPromises()
    await wrapper.get('#activity-config').setValue('activity-1')
    await wrapper.get('#activity-config').trigger('change')
    await wrapper.get('[data-testid="activity-start"]').trigger('click')
    await flushPromises()
    expect(device.startActivity).toHaveBeenCalledWith('activity-1', 1)

    device.getAppAppState.mockResolvedValueOnce({ ...readyState, activityConfigs: [activityConfig], activitySession: { ...readyState.activitySession, configId: 'activity-1', status: 'paused', pauseReason: { code: 'timeout', message: '超时' } } })
    const paused = mount(App)
    await flushPromises()
    const resumeButtons = paused.findAll('button').filter((button) => button.text().includes('重新识别'))
    await resumeButtons[0]!.trigger('click')
    await flushPromises()
    expect(device.previewActivityRecognition).toHaveBeenCalledWith('activity-1')
    const confirm = paused.findAll('button').find((button) => button.text().includes('确认恢复'))
    expect(confirm).toBeDefined()
    await confirm!.trigger('click')
    await flushPromises()
    expect(device.resumeActivity).toHaveBeenCalledOnce()
  })

  it('loads all six calibration states, renames, previews ambiguity, and deletes a config', async () => {
    device.getAppAppState.mockResolvedValueOnce({ ...readyState, activityConfigs: [activityConfig] })
    device.previewActivityRecognition.mockResolvedValueOnce({ matchedState: null, scores: pageStates.map((state) => ({ state, score: 0.5 })), reason: 'ambiguous' })
    vi.spyOn(window, 'confirm').mockReturnValueOnce(true)
    const wrapper = mount(App)
    await flushPromises()

    expect(wrapper.findAll('#calibration-state option').map((option) => option.attributes('value'))).toEqual(pageStates)
    await wrapper.get('#activity-config').setValue('activity-1')
    await wrapper.get('#activity-config').trigger('change')
    await wrapper.get('#activity-name').setValue('重命名活动')
    const save = wrapper.findAll('button').find((button) => button.text() === '保存配置')!
    expect((wrapper.get('#activity-config').element as HTMLSelectElement).value).toBe('activity-1')
    expect(save.attributes('disabled')).toBeUndefined()
    await save.trigger('click')
    await flushPromises()
    expect(device.saveActivityConfig).toHaveBeenCalledWith(expect.objectContaining({ id: 'activity-1', name: '重命名活动' }))

    const preview = wrapper.findAll('button').find((button) => button.text() === '预览识别评分')!
    await preview.trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('ambiguous')

    const remove = wrapper.findAll('button').find((button) => button.text() === '删除配置')!
    await remove.trigger('click')
    await flushPromises()
    expect(device.deleteActivityConfig).toHaveBeenCalledWith('activity-1')
  })

  it('calibrates recognition and safe actions for every required page state', async () => {
    device.calibrateActivityFeature.mockResolvedValue({ region: { left: 100, top: 50, width: 200, height: 100 }, signature: Array(48).fill(12) })
    const wrapper = mount(App)
    await flushPromises()
    await wrapper.get('[data-testid="capture-button"]').trigger('click')
    await flushPromises()
    const image = wrapper.get('img').element as HTMLImageElement
    Object.defineProperties(image, { naturalWidth: { configurable: true, value: 1000 }, naturalHeight: { configurable: true, value: 500 } })
    await wrapper.get('img').trigger('load')
    const stage = wrapper.get('[data-testid="screenshot-stage"]')
    vi.spyOn(stage.element, 'getBoundingClientRect').mockReturnValue({ left: 0, top: 0, width: 1000, height: 500, right: 1000, bottom: 500, x: 0, y: 0, toJSON: () => ({}) })
    await stage.trigger('mousedown', { clientX: 100, clientY: 50 })
    await stage.trigger('mouseup', { clientX: 300, clientY: 150 })
    await wrapper.findAll('button').find((button) => button.text() === '新建配置')!.trigger('click')

    for (const state of pageStates) {
      await wrapper.get('#calibration-state').setValue(state)
      await wrapper.findAll('button').find((button) => button.text() === '添加识别区域')!.trigger('click')
      await flushPromises()
      if (state !== 'battling') {
        await wrapper.findAll('button').find((button) => button.text() === '设为动作区域')!.trigger('click')
      }
    }
    await wrapper.findAll('button').find((button) => button.text() === '保存配置')!.trigger('click')
    await flushPromises()

    const saved = device.saveActivityConfig.mock.calls[0]![0] as ActivityConfig
    expect(saved.states).toHaveLength(6)
    expect(saved.states.every((profile) => profile.features.length === 1)).toBe(true)
    expect(saved.states.filter((profile) => profile.state !== 'battling').every((profile) => profile.action)).toBe(true)
    expect(saved.states.find((profile) => profile.state === 'battling')?.action).toBeNull()
  })

  it('surfaces config validation and prevents duplicate pause or stop submissions', async () => {
    device.getAppAppState.mockResolvedValueOnce({
      ...readyState,
      activityConfigs: [activityConfig],
      activitySession: { ...readyState.activitySession, configId: 'activity-1', status: 'navigating', targetRuns: 1 },
    })
    let resolvePause!: (value: AppState['activitySession']) => void
    device.pauseActivity.mockReturnValueOnce(new Promise((resolve) => { resolvePause = resolve }))
    const wrapper = mount(App)
    await flushPromises()
    const pause = wrapper.findAll('button').find((button) => button.text() === '暂停')!
    await pause.trigger('click')
    await pause.trigger('click')
    expect(device.pauseActivity).toHaveBeenCalledOnce()
    resolvePause({ ...readyState.activitySession, configId: 'activity-1', status: 'paused', targetRuns: 1, pauseReason: { code: 'user', message: '用户暂停' } })
    await flushPromises()

    let resolveStop!: (value: AppState['activitySession']) => void
    device.stopActivity.mockReturnValueOnce(new Promise((resolve) => { resolveStop = resolve }))
    const stop = wrapper.findAll('button').find((button) => button.text() === '停止')!
    await stop.trigger('click')
    await stop.trigger('click')
    expect(device.stopActivity).toHaveBeenCalledOnce()
    resolveStop(readyState.activitySession)
    await flushPromises()

    device.getAppAppState.mockResolvedValueOnce({ ...readyState, activityConfigs: [activityConfig] })
    device.saveActivityConfig.mockRejectedValueOnce({ code: 'ACTIVITY_CONFIG_INVALID', message: '配置时序无效', recovery: '修正范围' })
    const editor = mount(App)
    await flushPromises()
    await editor.get('#activity-config').setValue('activity-1')
    await editor.get('#activity-config').trigger('change')
    await editor.findAll('button').find((button) => button.text() === '保存配置')!.trigger('click')
    await flushPromises()
    expect(editor.get('.status-bar').text()).toContain('配置时序无效')
    expect(editor.get('.status-bar').text()).toContain('修正范围')
  })

  it('automatically advances a running task once and stops scheduling after completion', async () => {
    vi.useFakeTimers()
    try {
      device.getAppAppState.mockResolvedValueOnce({ ...readyState, activityConfigs: [activityConfig] })
      device.startActivity.mockResolvedValueOnce({ ...readyState.activitySession, configId: 'activity-1', status: 'navigating', targetRuns: 1 })
      const wrapper = mount(App)
      await flushPromises()
      await wrapper.get('#activity-config').setValue('activity-1')
      await wrapper.get('[data-testid="activity-start"]').trigger('click')
      await flushPromises()

      await vi.advanceTimersByTimeAsync(1200)
      await flushPromises()
      expect(device.advanceActivity).toHaveBeenCalledOnce()
      await vi.advanceTimersByTimeAsync(2400)
      expect(device.advanceActivity).toHaveBeenCalledOnce()
      wrapper.unmount()
    } finally {
      vi.useRealTimers()
    }
  })
})
