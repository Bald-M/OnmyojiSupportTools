export interface Point {
  x: number
  y: number
}

export interface Rect {
  left: number
  top: number
  width: number
  height: number
}

export interface Size {
  width: number
  height: number
}

export function calculateContainedImageRect(container: Rect, image: Size): Rect {
  if (container.width <= 0 || container.height <= 0 || image.width <= 0 || image.height <= 0) {
    return { left: container.left, top: container.top, width: 0, height: 0 }
  }

  const scale = Math.min(container.width / image.width, container.height / image.height)
  const width = image.width * scale
  const height = image.height * scale

  return {
    left: container.left + (container.width - width) / 2,
    top: container.top + (container.height - height) / 2,
    width,
    height,
  }
}

export function mapClientPointToImage(
  pointer: Point,
  container: Rect,
  image: Size,
): Point | null {
  const fitted = calculateContainedImageRect(container, image)
  const right = fitted.left + fitted.width
  const bottom = fitted.top + fitted.height

  if (
    fitted.width === 0 ||
    fitted.height === 0 ||
    pointer.x < fitted.left ||
    pointer.x > right ||
    pointer.y < fitted.top ||
    pointer.y > bottom
  ) {
    return null
  }

  const x = Math.round(((pointer.x - fitted.left) / fitted.width) * image.width)
  const y = Math.round(((pointer.y - fitted.top) / fitted.height) * image.height)

  return {
    x: Math.min(image.width - 1, Math.max(0, x)),
    y: Math.min(image.height - 1, Math.max(0, y)),
  }
}
