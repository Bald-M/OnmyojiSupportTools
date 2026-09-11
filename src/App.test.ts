import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import App from './App.vue'
import type { AppState } from './types/device'

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
}

describe('desktop device workflow', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    device.getAppAppState.mockResolvedValue(readyState)
    device.captureScreen.mockResolvedValue(new Uint8Array([137, 80, 78, 71]))
    device.tapScreen.mockResolvedValue({
      deviceSerial: '127.0.0.1:16384',
      point: { x: 640, y: 360 },
      completedAt: 1,
    })
    device.startPreview.mockResolvedValue({ ...readyState, previewDeviceSerial: readyState.activeDeviceSerial })
    device.stopPreview.mockResolvedValue(readyState)
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
    expect(device.tapScreen).toHaveBeenCalledWith({ x: 640, y: 360 })
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

    expect(device.tapScreen).toHaveBeenCalledWith({ x: 32, y: 48 })

    await wrapper.get('[data-testid="capture-button"]').trigger('click')
    await flushPromises()
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:test-screenshot')

    wrapper.unmount()
    expect(URL.revokeObjectURL).toHaveBeenCalledTimes(2)
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
})
