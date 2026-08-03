<script setup lang="ts">
import { getCurrentWebview } from '@tauri-apps/api/webview'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { computed, onActivated, onDeactivated, onMounted, ref } from 'vue'
import { getCompressConcurrency, compressVideo, prepareCompressBatch } from '../compress'
import {
  isTauri,
  pickOutputDirectory,
  pickVideoFiles,
  videosFromPaths,
  type PickedVideo,
} from '../desktop'
import { goToPage } from '../navigation'
import {
  activeBatchTaskId,
  cancelTaskLocal,
  completeTask,
  createTask,
  failTask,
  registerAbortHandler,
  tasks,
  unregisterAbortHandler,
  updateTaskProgress,
} from '../taskStore'
import type { QueueItem } from '../types'
import { formatSize, makeId, toBatchName, isVideoFileName } from '../utils'

const VIDEO_ACCEPT = 'video/*,.mp4,.mov,.mkv,.avi,.webm,.m4v,.wmv,.flv'

const items = ref<QueueItem[]>([])
const outputPath = ref('')
const isDragOver = ref(false)
const isCompressing = ref(false)
const dragDepth = ref(0)
const dropzoneEl = ref<HTMLElement | null>(null)
const runningInTauri = isTauri()
let abortCompress = false
let unlistenDragDrop: UnlistenFn | undefined
let lastDropKey = ''
let lastDropAt = 0

const pendingCount = computed(
  () => items.value.filter((i) => i.status === 'pending' || i.status === 'error').length,
)
const canCompress = computed(
  () =>
    items.value.length > 0 &&
    Boolean(outputPath.value) &&
    !isCompressing.value &&
    pendingCount.value > 0,
)

function isPointInDropzone(position: { x: number; y: number }) {
  const el = dropzoneEl.value
  if (!el) return false
  const rect = el.getBoundingClientRect()
  const scale = window.devicePixelRatio || 1
  const x = position.x / scale
  const y = position.y / scale
  return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom
}

async function bindDragDrop() {
  if (!runningInTauri || unlistenDragDrop) return
  unlistenDragDrop = await getCurrentWebview().onDragDropEvent(async (event) => {
    const payload = event.payload
    if (payload.type === 'enter' || payload.type === 'over') {
      isDragOver.value =
        payload.type === 'enter' ? true : isPointInDropzone(payload.position)
      return
    }
    if (payload.type === 'leave') {
      isDragOver.value = false
      return
    }
    if (payload.type !== 'drop') return

    isDragOver.value = false
    if (isCompressing.value) return

    const dropKey = payload.paths.join('\0')
    const now = Date.now()
    if (dropKey === lastDropKey && now - lastDropAt < 400) return
    lastDropKey = dropKey
    lastDropAt = now

    appendVideos(await videosFromPaths(payload.paths))
  })
}

function unbindDragDrop() {
  unlistenDragDrop?.()
  unlistenDragDrop = undefined
  isDragOver.value = false
}

onMounted(() => {
  void bindDragDrop()
})
onActivated(() => {
  void bindDragDrop()
})
onDeactivated(() => {
  unbindDragDrop()
})

function appendVideos(videos: PickedVideo[]) {
  if (!videos.length) return
  const existing = new Set(items.value.map((p) => p.path || `${p.name}-${p.size}`))
  const next = videos
    .filter((v) => !existing.has(v.path || `${v.name}-${v.size}`))
    .map((video) => ({
      id: makeId(),
      name: video.name,
      size: video.size,
      path: video.path,
      status: 'pending' as const,
      progress: 0,
      outputName: toBatchName(video.name),
    }))
  items.value = [...items.value, ...next]
}

function addBrowserFiles(fileList: FileList | File[]) {
  const videos = Array.from(fileList).filter(
    (f) => f.type.startsWith('video/') || isVideoFileName(f.name),
  )
  appendVideos(
    videos.map((file) => ({
      name: file.name,
      path: `browser://${file.name}-${file.size}-${file.lastModified}`,
      size: file.size,
    })),
  )
}

function removeItem(id: string) {
  if (isCompressing.value) return
  items.value = items.value.filter((i) => i.id !== id)
}

function clearAll() {
  if (isCompressing.value) return
  items.value = []
}

async function pickOutputDir() {
  const selected = await pickOutputDirectory(outputPath.value || undefined)
  if (selected) outputPath.value = selected
}

async function chooseVideos() {
  if (isCompressing.value) return
  if (runningInTauri) appendVideos(await pickVideoFiles())
}

function onFileChange(e: Event) {
  const input = e.target as HTMLInputElement
  if (input.files) addBrowserFiles(input.files)
  input.value = ''
}

async function startCompress() {
  if (!items.value.length || !outputPath.value || isCompressing.value) return
  if (!runningInTauri) {
    window.alert('真实压缩需要桌面应用。请运行：make dev')
    return
  }

  abortCompress = false
  isCompressing.value = true
  const queue = items.value.filter((i) => i.status === 'pending' || i.status === 'error')
  const outputDir = outputPath.value
  let cursor = 0
  let doneCount = 0
  let failCount = 0
  const itemProgress = new Map<string, number>()

  const taskId = await createTask({
    type: 'batch',
    title: '批量压制',
    meta: { videoCount: queue.length, outputDir },
  })
  activeBatchTaskId.value = taskId
  registerAbortHandler(taskId, () => {
    abortCompress = true
  })

  items.value = []
  goToPage('tasks')

  await prepareCompressBatch()

  function refreshTaskProgress() {
    if (abortCompress) return
    const partial = [...itemProgress.values()].reduce((a, b) => a + b, 0)
    const overall = ((doneCount + partial) / queue.length) * 100
    updateTaskProgress(taskId, Math.min(99, overall))
  }

  async function worker() {
    while (!abortCompress) {
      const index = cursor
      cursor += 1
      if (index >= queue.length) break
      const item = queue[index]
      itemProgress.set(item.id, 0)

      try {
        await compressVideo({
          id: item.id,
          inputPath: item.path,
          outputDir,
          outputName: item.outputName,
          onProgress: (progress) => {
            itemProgress.set(item.id, progress / 100)
            refreshTaskProgress()
          },
        })
        if (abortCompress) break
        itemProgress.delete(item.id)
        doneCount += 1
        refreshTaskProgress()
      } catch {
        itemProgress.delete(item.id)
        if (abortCompress) break
        failCount += 1
      }
    }
  }

  const pool = Math.min(getCompressConcurrency(), queue.length)
  await Promise.all(Array.from({ length: pool }, () => worker()))

  const current = tasks.value.find((t) => t.id === taskId)
  if (current?.status === 'cancelled') {
    // 已由任务列表取消
  } else if (abortCompress) {
    cancelTaskLocal(taskId)
  } else if (doneCount >= queue.length) {
    completeTask(taskId, { outputDir })
  } else {
    failTask(taskId, failCount > 0 ? '部分文件压制失败' : '任务未完成')
  }

  unregisterAbortHandler(taskId)
  if (activeBatchTaskId.value === taskId) activeBatchTaskId.value = null
  isCompressing.value = false
}

function onDragEnter(e: DragEvent) {
  e.preventDefault()
  dragDepth.value += 1
  isDragOver.value = true
}
function onDragLeave(e: DragEvent) {
  e.preventDefault()
  dragDepth.value -= 1
  if (dragDepth.value <= 0) {
    dragDepth.value = 0
    isDragOver.value = false
  }
}
function onDrop(e: DragEvent) {
  e.preventDefault()
  dragDepth.value = 0
  isDragOver.value = false
  if (e.dataTransfer?.files?.length) addBrowserFiles(e.dataTransfer.files)
}
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1>批量压制</h1>
        <p>批量选择视频，压缩体积，分辨率保持不变</p>
      </div>
      <div class="preset-pill">画质优先</div>
    </header>

    <section class="panel">
      <div
        ref="dropzoneEl"
        class="dropzone"
        :class="{ 'is-dragover': isDragOver, 'is-native': runningInTauri }"
        @dragenter="onDragEnter"
        @dragover.prevent
        @dragleave="onDragLeave"
        @drop="onDrop"
        @click="runningInTauri ? chooseVideos() : undefined"
      >
        <input
          v-if="!runningInTauri"
          class="file-input"
          type="file"
          :accept="VIDEO_ACCEPT"
          multiple
          :disabled="isCompressing"
          @change="onFileChange"
        />
        <h2>拖拽或选择多个视频</h2>
        <p>支持 mp4 / mov / mkv 等常见格式</p>
        <button type="button" class="btn btn-ghost" tabindex="-1" @click.stop="chooseVideos">
          选择文件
        </button>
      </div>

      <div v-if="items.length" class="list-section selection-section">
        <div class="list-head">
          <h3>已选视频</h3>
          <div class="list-head-actions">
            <span>{{ items.length }} 个文件</span>
            <button
              type="button"
              class="btn btn-ghost btn-sm"
              :disabled="isCompressing"
              @click="clearAll"
            >
              清空
            </button>
          </div>
        </div>
        <ul class="file-list selected-list">
          <li v-for="item in items" :key="item.id" class="file-item">
            <div class="file-meta">
              <p class="file-name" :title="item.name">{{ item.name }}</p>
              <p class="file-path" :title="item.path">{{ item.path }}</p>
              <p class="file-sub">{{ formatSize(item.size) }} · 输出 {{ item.outputName }}</p>
            </div>
            <div class="file-actions">
              <button
                type="button"
                class="btn btn-ghost btn-sm"
                :disabled="isCompressing"
                @click="removeItem(item.id)"
              >
                移除
              </button>
            </div>
          </li>
        </ul>
      </div>

      <div class="controls">
        <div class="output-row">
          <div class="output-field">
            <span class="output-label">输出文件夹</span>
            <p class="output-path" :class="{ 'is-placeholder': !outputPath }">
              {{ outputPath || '尚未选择，压制后的文件将保存到此处' }}
            </p>
          </div>
          <button type="button" class="btn btn-ghost" :disabled="isCompressing" @click="pickOutputDir">
            选择路径
          </button>
        </div>
        <div class="action-row">
          <p class="hint">
            进度在「任务列表」查看；输出名加 <code>_batch</code>；并行
            <code>{{ getCompressConcurrency() }}</code> 个
          </p>
          <button
            type="button"
            class="btn btn-primary"
            :disabled="!canCompress"
            @click="startCompress"
          >
            {{ isCompressing ? '压制中…' : '开始压制' }}
          </button>
        </div>
      </div>
    </section>
  </div>
</template>

<style scoped>
.dropzone.is-native {
  cursor: pointer;
}
.list-head-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}
.selection-section {
  padding-top: 14px;
}
</style>
