import { spawn } from 'node:child_process'
import console from 'node:console'
import { readFile } from 'node:fs/promises'
import path from 'node:path'
import process from 'node:process'
import { setTimeout as delay } from 'node:timers/promises'

import tauriConfig from '../src-tauri/tauri.conf.json' with { type: 'json' }

if (process.platform !== 'win32' || process.arch !== 'x64') {
  throw new Error('The Release diagnostics smoke test requires Windows x64')
}

const localAppData = process.env.LOCALAPPDATA
if (!localAppData) {
  throw new Error('LOCALAPPDATA is unavailable')
}

const binary = path.resolve('src-tauri/target/x86_64-pc-windows-msvc/release/onmyoji-support-tools.exe')
const logPath = path.join(localAppData, tauriConfig.identifier, 'logs', 'diagnostics.jsonl')
const startedAt = Date.now()
const app = spawn(binary, [], { cwd: path.dirname(binary), stdio: 'ignore', windowsHide: true })
let spawnError
app.once('error', (error) => { spawnError = error })

try {
  const deadline = Date.now() + 30_000
  let verified = false
  while (Date.now() < deadline) {
    if (spawnError) throw spawnError
    if (app.exitCode !== null || app.signalCode !== null) {
      throw new Error(`Release application exited before writing its startup log: ${app.exitCode ?? app.signalCode}`)
    }

    let lines = ''
    try {
      lines = await readFile(logPath, 'utf8')
    } catch (error) {
      if (error.code !== 'ENOENT') throw error
    }
    const completeLines = lines.slice(0, lines.lastIndexOf('\n') + 1)
    const startup = completeLines
      .split('\n')
      .filter(Boolean)
      .map((line) => JSON.parse(line))
      .find((entry) => entry.operation === 'app'
        && entry.step === 'startup'
        && Number(entry.timestampMs) >= startedAt - 1_000)

    if (startup) {
      if (startup.metadata?.version !== tauriConfig.version
        || startup.metadata?.platform !== 'windows'
        || startup.metadata?.architecture !== 'x86_64'
        || !/^[0-9a-f]{32}$/.test(startup.sessionId)) {
        throw new Error('Release startup log is missing expected version, platform, architecture, or session ID')
      }
      console.log('Verified Windows x64 Release startup log in the Tauri app log directory')
      verified = true
      break
    }
    await delay(500)
  }
  if (!verified) {
    throw new Error('Release startup log did not appear in the Tauri app log directory')
  }
} finally {
  if (app.exitCode === null && app.signalCode === null) app.kill()
}
