import { afterEach, describe, expect, it, vi } from 'vitest'

import { AnnexBAccessUnitParser, H264CanvasDecoder } from './h264'

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('Annex B H.264 access-unit parser', () => {
  it('keeps split NAL units until the next start code arrives', () => {
    const parser = new AnnexBAccessUnitParser()

    expect(parser.push(new Uint8Array([0, 0, 0, 1, 0x67, 0x64]))).toEqual([])
    expect(parser.push(new Uint8Array([0, 0x1f, 0, 0]))).toEqual([])
    expect(parser.push(new Uint8Array([1, 0x68, 1, 2, 0, 0, 1, 9, 0xf0]))).toEqual([])
    expect(parser.codec).toBe('avc1.64001f')
  })

  it('emits complete access units and identifies IDR frames as key frames', () => {
    const parser = new AnnexBAccessUnitParser()
    const bytes = new Uint8Array([
      0, 0, 0, 1, 0x67, 0x64, 0, 0x1f,
      0, 0, 0, 1, 0x68, 1,
      0, 0, 1, 9, 0xf0,
      0, 0, 1, 0x65, 1, 2,
      0, 0, 1, 9, 0xf0,
      0, 0, 1, 0x41, 3, 4,
      0, 0, 1, 9, 0xf0,
      0, 0, 1, 6, 1,
    ])

    const units = parser.push(bytes)

    expect(units).toHaveLength(2)
    expect(units[0]?.type).toBe('key')
    expect(units[1]?.type).toBe('delta')
    expect(Array.from(units[0]?.data.slice(0, 5) ?? [])).toEqual([0, 0, 0, 1, 0x67])
  })

  it('uses first_mb_in_slice when a stream omits access-unit delimiters', () => {
    const parser = new AnnexBAccessUnitParser()
    const units = parser.push(new Uint8Array([
      0, 0, 1, 0x67, 0x64, 0, 0x1f,
      0, 0, 1, 0x68, 1,
      0, 0, 1, 0x65, 0x80,
      0, 0, 1, 0x41, 0x80,
      0, 0, 1, 0x41, 0x80,
    ]))

    expect(units).toHaveLength(1)
    expect(units[0]?.type).toBe('key')
  })
})

it('reports the average rate of frames actually drawn to the canvas', () => {
  let output: VideoDecoderInit['output'] = () => {}
  vi.stubGlobal('VideoDecoder', class {
    state = 'configured'
    constructor(callbacks: VideoDecoderInit) { output = callbacks.output }
    close() { this.state = 'closed' }
  })
  let now = 0
  vi.spyOn(performance, 'now').mockImplementation(() => now)
  const drawImage = vi.fn()
  const canvas = { getContext: () => ({ drawImage }), width: 0, height: 0 } as unknown as HTMLCanvasElement
  const onRate = vi.fn()
  const decoder = new H264CanvasDecoder(canvas, vi.fn(), onRate)
  const frame = { displayWidth: 1280, displayHeight: 720, close: vi.fn() } as unknown as VideoFrame

  output(frame)
  now = 500
  output(frame)
  expect(onRate).not.toHaveBeenCalled()
  now = 1000
  output(frame)

  expect(drawImage).toHaveBeenCalledTimes(3)
  expect(frame.close).toHaveBeenCalledTimes(3)
  expect(onRate).toHaveBeenCalledWith(2, 3)
  decoder.close()
})
