import { describe, expect, it } from 'vitest'

import { calculateContainedImageRect, mapClientPointToImage } from './coordinates'

describe('coordinate mapping', () => {
  it('maps a click through horizontal letterboxing', () => {
    const container = { left: 100, top: 50, width: 1000, height: 700 }
    const image = { width: 1280, height: 720 }

    expect(calculateContainedImageRect(container, image)).toEqual({
      left: 100,
      top: 118.75,
      width: 1000,
      height: 562.5,
    })
    expect(mapClientPointToImage({ x: 600, y: 400 }, container, image)).toEqual({
      x: 640,
      y: 360,
    })
  })

  it('returns null when the pointer is in the letterbox area', () => {
    const container = { left: 0, top: 0, width: 800, height: 800 }
    const image = { width: 1280, height: 720 }

    expect(mapClientPointToImage({ x: 400, y: 80 }, container, image)).toBeNull()
  })

  it('clamps the rounded coordinate to the final image pixel', () => {
    const container = { left: 0, top: 0, width: 1280, height: 720 }
    const image = { width: 1280, height: 720 }

    expect(mapClientPointToImage({ x: 1279.9, y: 719.9 }, container, image)).toEqual({
      x: 1279,
      y: 719,
    })
  })
})
