import { execFile, spawn } from 'child_process'
import { promisify } from 'util'
const execFileAsync = promisify(execFile)

const adbPath = '"D:\\Program Files (x86)\\MuMu Player 12\\shell\\adb.exe"'
const deviceId = '127.0.0.1:16384'

export async function connectDevice(ip: string, port: string): Promise<string> {
  const address = `${ip}:${port}`
  const { stdout } = await execFileAsync(adbPath, ['connect', address], { shell: true })
  return stdout.trim()
}

export async function takeScreenshot(): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const adb = spawn(
      adbPath,
      ['-s', deviceId, 'exec-out', 'screencap', '-p'],
      {
        shell: true,
      }
    )

    const chunks: Buffer[] = []
    const errors: Buffer[] = []

    adb.stdout.on('data', (chunk) => chunks.push(chunk))
    adb.stderr.on('data', (err) => errors.push(err))

    adb.on('close', async (code) => {
      if (code === 0) {
        const imageBuffer = Buffer.concat(chunks)

        const fixedBuffer = Buffer.from(
          imageBuffer.toString('binary').replace(/\r\r\n/g, '\n'),
          'binary'
        )

        const header = fixedBuffer.subarray(0, 4)
        const pngSignature = Buffer.from([0x89, 0x50, 0x4e, 0x47])

        if (!header.equals(pngSignature)) {
          return reject(new Error('Fixed buffer is not a valid PNG image'))
        }

        resolve(imageBuffer)
      } else {
        reject(new Error(Buffer.concat(errors).toString()))
      }
    })

    adb.on('error', (err) => reject(err))
  })
}

export async function tap(x: number, y: number): Promise<void> {
  try {
    await execFileAsync(adbPath, ['-s', deviceId, 'shell', 'input', 'tap', String(x), String(y)], {
      shell: true
    })
  } catch (err) {
    throw new Error(`Failed to tap at (${x}, ${y}): ${(err as Error).message}`)
  }
}
