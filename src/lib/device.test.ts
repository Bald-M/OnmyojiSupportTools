import { beforeEach, describe, expect, it, vi } from 'vitest'

const invoke = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ invoke, Channel: class {} }))

import { refreshDevices } from './device'

describe('diagnostic IPC correlation', () => {
  beforeEach(() => invoke.mockReset())

  it('uses one trace ID for an IPC failure and its frontend report', async () => {
    invoke.mockImplementation((command: string) => command === 'refresh_devices'
      ? Promise.reject({ code: 'ADB_TIMEOUT', message: 'private device serial' })
      : Promise.resolve())

    await expect(refreshDevices()).rejects.toMatchObject({ code: 'ADB_TIMEOUT' })
    await Promise.resolve()
    const request = invoke.mock.calls.find(([command]) => command === 'refresh_devices')!
    const report = invoke.mock.calls.find(([command]) => command === 'report_frontend_error')!
    expect(request[1].traceId).toMatch(/^[0-9a-f]{32}$/)
    expect(report[1]).toEqual({ code: 'IPC_FAILED', traceId: request[1].traceId })
    expect(JSON.stringify(report[1])).not.toContain('private device serial')
  })
})
