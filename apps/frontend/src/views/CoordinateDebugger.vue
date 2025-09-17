<template>
  <div class="debugger">
    <h2>屏幕调试器</h2>
    <div class="screenshot-wrapper">
      <img
        :src="screenshotUrl"
        @click="handleClick"
        ref="image"
        alt="device screenshot"
      />
    </div>
    <p>点击坐标：{{ coords.x }}, {{ coords.y }}</p>
    <!-- <button @click="screenshot">Screenshot</button> -->
    <!-- <button @click="sendTap">发送 Tap 到后端</button> -->
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { emulatorScreenshotService } from '@/api/emulator'


const screenshotUrl = ref('http://127.0.0.1:3000/device/screenshot') // 避免缓存
const coords = ref({ x: 0, y: 0 })
const image = ref<HTMLImageElement | null>(null)

function handleClick(e: MouseEvent) {
  const rect = image.value!.getBoundingClientRect()
  const scaleX = image.value!.naturalWidth / rect.width
  const scaleY = image.value!.naturalHeight / rect.height

  coords.value.x = Math.round((e.clientX - rect.left) * scaleX)
  coords.value.y = Math.round((e.clientY - rect.top) * scaleY)
}

// async function sendTap() {
//   await axios.get('/api/device/test').then(res => {
//     console.log(res)
//     alert(res.status)
//   })
// }

// async function screenshot() {
//   await emulatorScreenshotService()
// }

</script>

<style scoped>
.debugger {
  max-width: 720px;
  margin: auto;
}
.screenshot-wrapper img {
  width: 100%;
  border: 1px solid #ccc;
  cursor: crosshair;
}
</style>
