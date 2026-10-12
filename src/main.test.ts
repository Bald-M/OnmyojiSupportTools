import { expect, it, vi } from 'vitest'

const reportFrontendError = vi.hoisted(() => vi.fn())
const app = vi.hoisted(() => ({ config: { errorHandler: undefined as undefined | (() => void) }, mount: vi.fn() }))
vi.mock('vue', () => ({ createApp: () => app }))
vi.mock('./App.vue', () => ({ default: {} }))
vi.mock('./lib/device', () => ({ reportFrontendError }))

it('reports unhandled Vue and browser errors without their messages', async () => {
  await import('./main')
  app.config.errorHandler?.()
  window.dispatchEvent(new Event('error'))
  window.dispatchEvent(new Event('unhandledrejection'))
  expect(reportFrontendError.mock.calls).toEqual([
    ['FRONTEND_UNHANDLED'], ['FRONTEND_UNHANDLED'], ['FRONTEND_REJECTION'],
  ])
})
