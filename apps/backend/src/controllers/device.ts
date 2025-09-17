import { Context } from 'koa'
import * as DeviceService from '../services/device'


export async function connect(ctx: Context) {
  const { ip, port } = ctx.request.body as { ip?: string; port?: string }

  if (!ip || !port) {
    ctx.status = 400
    ctx.body = { success: false, error: 'Missing ip or port' }
    return
  }

  try {
    const result = await DeviceService.connectDevice(ip, port)
    ctx.body = { success: true, result }
  } catch (err) {
    ctx.status = 500
    ctx.body = {
      success: false,
      error: 'Failed to connect device',
      detail: (err as Error).message,
    }
  }
}

export async function screenshot(ctx: Context) {
  try {
    const buffer = await DeviceService.takeScreenshot()
    ctx.type = 'image/png'
    ctx.body = buffer
  } catch (err) {
    ctx.status = 500
    ctx.body = { 
      error: 'Screenshot failed',
      msg: typeof err === 'object' && err !== null && 'message' in err ? (err as any).message : String(err),
      detail: JSON.stringify(err)
    }
  }
}

export async function tap(ctx: Context) {
  const body = ctx.request.body as { x: number; y: number }
  const { x, y } = body

  if (typeof x !== 'number' || typeof y !== 'number') {
    ctx.status = 400
    ctx.body = { error: 'x and y must be numbers' }
    return
  }

  try {
    await DeviceService.tap(x, y)
    ctx.body = { success: true, message: `Tapped at (${x}, ${y})` }
  } catch (err) {
    ctx.status = 500
    ctx.body = {
      error: 'Tap failed',
      detail: (err as Error).message
    }
  }
}