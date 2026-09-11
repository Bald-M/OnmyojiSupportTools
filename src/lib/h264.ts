export interface H264AccessUnit {
  data: Uint8Array
  type: EncodedVideoChunkType
}

function startCodes(bytes: Uint8Array): number[] {
  const offsets: number[] = []
  for (let index = 0; index + 3 < bytes.length; index += 1) {
    if (
      bytes[index] === 0 &&
      bytes[index + 1] === 0 &&
      (bytes[index + 2] === 1 || (bytes[index + 2] === 0 && bytes[index + 3] === 1))
    ) {
      offsets.push(index)
      index += bytes[index + 2] === 1 ? 2 : 3
    }
  }
  return offsets
}

function nalHeaderIndex(nal: Uint8Array): number {
  return nal[2] === 1 ? 3 : 4
}

function firstMacroblockIndex(nal: Uint8Array): number | null {
  const header = nalHeaderIndex(nal)
  const rbsp: number[] = []
  for (let index = header + 1; index < nal.length; index += 1) {
    if (index >= header + 3 && nal[index] === 3 && nal[index - 1] === 0 && nal[index - 2] === 0) {
      continue
    }
    rbsp.push(nal[index] ?? 0)
  }
  let leadingZeroBits = 0
  const bitLength = rbsp.length * 8
  const bitAt = (index: number) => ((rbsp[Math.floor(index / 8)] ?? 0) >> (7 - (index % 8))) & 1
  while (leadingZeroBits < bitLength && bitAt(leadingZeroBits) === 0) leadingZeroBits += 1
  if (leadingZeroBits >= bitLength || leadingZeroBits > 30) return null
  let value = 0
  for (let index = 1; index <= leadingZeroBits; index += 1) {
    value = (value << 1) | bitAt(leadingZeroBits + index)
  }
  return (1 << leadingZeroBits) - 1 + value
}

function concatenate(parts: Uint8Array[]): Uint8Array<ArrayBuffer> {
  const result = new Uint8Array(parts.reduce((length, part) => length + part.length, 0))
  let offset = 0
  for (const part of parts) {
    result.set(part, offset)
    offset += part.length
  }
  return result
}

export class AnnexBAccessUnitParser {
  private pending = new Uint8Array()
  private parameterSets: Uint8Array[] = []
  private current: Uint8Array[] = []
  private currentHasVcl = false
  private currentIsKey = false
  codec: string | null = null

  push(chunk: Uint8Array): H264AccessUnit[] {
    this.pending = concatenate([this.pending, chunk])
    const offsets = startCodes(this.pending)
    if (offsets.length < 2) return []

    const emitted: H264AccessUnit[] = []
    for (let index = 0; index < offsets.length - 1; index += 1) {
      const nal = this.pending.slice(offsets[index], offsets[index + 1])
      this.consumeNal(nal, emitted)
    }
    this.pending = this.pending.slice(offsets[offsets.length - 1])
    return emitted
  }

  private consumeNal(nal: Uint8Array, emitted: H264AccessUnit[]) {
    const header = nalHeaderIndex(nal)
    const type = (nal[header] ?? 0) & 0x1f
    if (type === 7) {
      this.parameterSets = this.parameterSets.filter((item) => ((item[nalHeaderIndex(item)] ?? 0) & 0x1f) !== 7)
      this.parameterSets.unshift(nal)
      if (nal.length >= header + 4) {
        this.codec = `avc1.${[nal[header + 1], nal[header + 2], nal[header + 3]]
          .map((value) => value?.toString(16).padStart(2, '0'))
          .join('')}`
      }
      return
    }
    if (type === 8) {
      this.parameterSets = this.parameterSets.filter((item) => ((item[nalHeaderIndex(item)] ?? 0) & 0x1f) !== 8)
      this.parameterSets.push(nal)
      return
    }
    if (type === 9) {
      this.emitCurrent(emitted)
      this.current = [nal]
      return
    }

    if (type === 1 || type === 5) {
      if (this.currentHasVcl && firstMacroblockIndex(nal) === 0) this.emitCurrent(emitted)
      if (!this.currentHasVcl && type === 5) this.current.unshift(...this.parameterSets)
      this.currentHasVcl = true
      this.currentIsKey ||= type === 5
    }
    this.current.push(nal)
  }

  private emitCurrent(emitted: H264AccessUnit[]) {
    if (this.currentHasVcl) {
      emitted.push({
        data: concatenate(this.current),
        type: this.currentIsKey ? 'key' : 'delta',
      })
    }
    this.current = []
    this.currentHasVcl = false
    this.currentIsKey = false
  }
}

export class H264CanvasDecoder {
  private readonly parser = new AnnexBAccessUnitParser()
  private readonly decoder: VideoDecoder
  private configuredCodec: string | null = null
  private timestamp = 0
  private waitingForKeyFrame = false

  constructor(canvas: HTMLCanvasElement, onError: (error: Error) => void) {
    const context = canvas.getContext('2d')
    if (!context || typeof VideoDecoder === 'undefined') {
      throw new Error('当前 WebView2 不支持 H.264 WebCodecs 解码')
    }
    this.decoder = new VideoDecoder({
      output: (frame) => {
        canvas.width = frame.displayWidth
        canvas.height = frame.displayHeight
        context.drawImage(frame, 0, 0, canvas.width, canvas.height)
        frame.close()
      },
      error: (error) => {
        this.waitingForKeyFrame = true
        onError(error)
      },
    })
  }

  push(chunk: Uint8Array) {
    for (const unit of this.parser.push(chunk)) {
      const codec = this.parser.codec
      if (!codec) continue
      if (codec !== this.configuredCodec) {
        this.decoder.configure({ codec, optimizeForLatency: true, hardwareAcceleration: 'prefer-hardware' })
        this.configuredCodec = codec
        this.waitingForKeyFrame = true
      }
      if (this.decoder.decodeQueueSize > 2) {
        this.decoder.reset()
        this.decoder.configure({ codec, optimizeForLatency: true, hardwareAcceleration: 'prefer-hardware' })
        this.waitingForKeyFrame = true
      }
      if (this.waitingForKeyFrame && unit.type !== 'key') continue
      this.waitingForKeyFrame = false
      this.decoder.decode(new EncodedVideoChunk({
        type: unit.type,
        timestamp: this.timestamp,
        duration: 33_333,
        data: unit.data,
      }))
      this.timestamp += 33_333
    }
  }

  close() {
    if (this.decoder.state !== 'closed') this.decoder.close()
  }
}
