/* global document, window */
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import console from 'node:console'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import process from 'node:process'
import { URL, fileURLToPath, pathToFileURL } from 'node:url'
import vue from '@vitejs/plugin-vue'
import { build } from 'vite'

// Optional real-browser QA: provide Playwright's module and Chromium executable.
const { chromium } = await import(process.argv[2] ? pathToFileURL(process.argv[2]).href : 'playwright')
const root = fileURLToPath(new URL('../', import.meta.url))
const outputDirectory = join(root, 'docs/qa/issue-24')
const temporaryDirectory = mkdtempSync(join(tmpdir(), 'region-editor-qa-'))
const baseline = process.argv.includes('--before')
const appSource = readFileSync(join(root, 'src/App.vue'), 'utf8')
const originalFile = path => execFileSync('git', ['show', `b40208c:${path}`], { cwd: root, encoding: 'utf8' })
const emptyState = appSource.match(/const emptyState: AppState = (\{[\s\S]*?\n\})\n/)[1]
const deviceImports = appSource.match(/import \{([^{}]*?)\} from '\.\/lib\/device'/)[1]
const otherCommands = deviceImports.split(',').map(name => name.trim()).filter(name => /^\w+$/.test(name) && !['captureScreen', 'getAppAppState', 'tapScreen'].includes(name))
// Synthetic black/white pixels only; no ADB, real device, account, or game assets.
const deviceStub = `
const state = ${emptyState};
state.selectedAdb = { path: 'adb.exe', version: '37.0.1', source: 'bundled' };
state.activeDeviceSerial = 'test-device';
state.devices = [{ serial: 'test-device', model: 'Synthetic QA', status: 'online', transport: '1' }];
window.tapCount = 0;
export async function getAppAppState() { return state }
export async function captureScreen() {
  const canvas = document.createElement('canvas'); canvas.width = 1000; canvas.height = 500;
  const context = canvas.getContext('2d');
  context.fillStyle = '#fafafa'; context.fillRect(0, 0, 500, 500);
  context.fillStyle = '#151515'; context.fillRect(500, 0, 500, 500);
  const blob = await new Promise(resolve => canvas.toBlob(resolve, 'image/png'));
  return new Uint8Array(await blob.arrayBuffer());
}
export async function tapScreen() { window.tapCount++; return { finalPoint: { x: 0, y: 0 } } }
${otherCommands.map(name => `export async function ${name}() { return state }`).join('\n')}
`
let browser
try {
  const result = await build({
    root, configFile: false, define: { 'process.env.NODE_ENV': JSON.stringify('production') },
    plugins: [{
      name: 'region-qa', enforce: 'pre',
      resolveId(source) { if (source.endsWith('qa-entry')) return '\0qa-entry' },
      load(id) {
        if (id === '\0qa-entry') return `import {createApp} from 'vue'; import App from ${JSON.stringify(join(root, 'src/App.vue').replaceAll('\\', '/'))}; createApp(App).mount('#app')`
        if (id.endsWith('/src/lib/device.ts')) return deviceStub
        if (baseline && id.endsWith('/src/App.vue')) return originalFile('src/App.vue')
      },
    }, vue()],
    build: { write: false, lib: { entry: 'qa-entry', name: 'QA', formats: ['iife'] }, minify: false },
  })
  const javascript = (Array.isArray(result) ? result[0] : result).output.find(item => item.type === 'chunk').code
  const css = baseline ? originalFile('src/styles.css') : readFileSync(join(root, 'src/styles.css'), 'utf8')
  const htmlPath = join(temporaryDirectory, 'qa.html')
  writeFileSync(htmlPath, `<html><meta charset="utf-8"><style>${css}</style><div id="app"></div><script>${javascript}</script></html>`)
  browser = await chromium.launch({ headless: true, ...(process.argv[3] ? { executablePath: process.argv[3] } : {}) })
  const page = await browser.newPage({ viewport: { width: 1280, height: 1000 } })
  page.on('pageerror', error => { throw error })
  await page.goto(pathToFileURL(htmlPath).href)
  await page.getByTestId('capture-button').click()
  await page.waitForFunction(() => document.querySelector('img')?.naturalWidth === 1000)
  mkdirSync(outputDirectory, { recursive: true })
  if (baseline) {
    await page.getByTestId('screenshot-stage').click()
    await page.getByRole('button', { name: '从当前点创建可编辑区域', exact: true }).click()
    await page.locator('.workspace').screenshot({ path: join(outputDirectory, 'before.png') })
  } else {
    await page.getByRole('button', { name: '创建区域', exact: true }).click()
    const stage = await page.getByTestId('screenshot-stage').boundingBox()
    const center = { x: stage.x + stage.width / 2, y: stage.y + stage.height / 2 }
    const values = () => page.locator('[aria-label="点击区域像素边界"] input').evaluateAll(inputs => inputs.map(input => Number(input.value)))
    const drag = async (start, end) => {
      await page.mouse.move(start.x, start.y)
      await page.mouse.down()
      await page.mouse.move(end.x, end.y, { steps: 5 })
      await page.mouse.up()
    }
    const controls = ['n', 'ne', 'e', 'se', 's', 'sw', 'w', 'nw']
    const controlSelector = handle => handle === 'move' ? '[data-selection-move]' : `[data-resize-handle="${handle}"]`
    await drag({ x: center.x - 10, y: center.y - 10 }, { x: center.x + 10, y: center.y + 10 })
    assert.deepEqual(await page.getByTestId('screenshot-stage').boundingBox(), stage, 'Layout remains stable during creation')
    const initial = await values()
    assert.equal(initial[2], initial[3], 'Equal screen distances map to equal device dimensions')
    assert(initial[2] > 0 && initial[2] < 40, 'Small region survives the full native pointer/mouse/click sequence')
    const boxes = await Promise.all([...controls, 'move'].map(handle => page.locator(controlSelector(handle)).boundingBox()))
    for (let i = 0; i < boxes.length; i++) for (let j = i + 1; j < boxes.length; j++) {
      const a = boxes[i], b = boxes[j]
      assert(!(a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height), 'Small-region controls never overlap')
    }
    for (const handle of controls) {
      const before = await values()
      const box = await page.locator(controlSelector(handle)).boundingBox()
      const start = { x: box.x + box.width / 2, y: box.y + box.height / 2 }
      await drag(start, { x: start.x + (handle.includes('w') ? -8 : handle.includes('e') ? 8 : 0), y: start.y + (handle.includes('n') ? -8 : handle.includes('s') ? 8 : 0) })
      assert.notDeepEqual(await values(), before, `Native hit testing reaches ${handle}`)
    }
    const beforeMove = await values()
    const move = await page.locator(controlSelector('move')).boundingBox()
    await drag({ x: move.x + 12, y: move.y + 12 }, { x: stage.x + stage.width + 100, y: stage.y + stage.height + 100 })
    const afterMove = await values()
    assert.deepEqual(afterMove.slice(2), beforeMove.slice(2), 'Moving preserves size')
    assert.deepEqual(afterMove.slice(0, 2), [1000 - afterMove[2], 500 - afterMove[3]], 'Outside release clamps to device edges')
    await page.mouse.move(center.x, center.y)
    assert.deepEqual(await values(), afterMove, 'Outside release ends pointer capture')
    assert.equal(await page.evaluate(() => window.tapCount), 0, 'Editing never sends device input')
    const fields = page.locator('[aria-label="点击区域像素边界"] input')
    for (const [index, value] of [[0, '440'], [1, '200']]) { await fields.nth(index).fill(value); await fields.nth(index).blur() }
    await page.locator('.workspace').screenshot({ path: join(outputDirectory, 'region-editor.png') })
    await page.emulateMedia({ colorScheme: 'dark' })
    await page.locator('.workspace').screenshot({ path: join(outputDirectory, 'region-editor-dark.png') })
    console.log('PASS: native Chromium hit testing, eight handles, stable mapping, outside release, zero device input')
  }
} finally {
  await browser?.close()
  rmSync(temporaryDirectory, { recursive: true, force: true })
}
