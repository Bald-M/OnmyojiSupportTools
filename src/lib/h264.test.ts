import { describe, expect, it } from 'vitest'

import { AnnexBAccessUnitParser } from './h264'

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
