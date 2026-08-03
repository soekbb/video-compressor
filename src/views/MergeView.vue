<script setup lang="ts">
import { getCurrentWebview } from '@tauri-apps/api/webview'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { computed, onActivated, onDeactivated, onMounted, ref } from 'vue'
import { prepareCompressBatch } from '../compress'
import {
  isTauri,
  pickOutputDirectory,
  pickVideoFiles,
  videosFromPaths,
  type PickedVideo,
} from '../desktop'
import { mergeVideos } from '../merge'
import { goToPage } from '../navigation'
import {
  activeMergeTaskId,
  cancelTaskLocal,
  completeTask,
  createTask,
  failTask,
  registerAbortHandler,
  tasks,
  unregisterAbortHandler,
  updateTaskProgress,
} from '../taskStore'
import { formatSize, makeId, isVideoFileName } from '../utils'

type MergeItem = PickedVideo & { id: string }

const VIDEO_ACCEPT = 'video/*,.mp4,.mov,.mkv,.avi,.webm,.m4v,.wmv,.flv'

const items = ref<MergeItem[]>([])
const outputPath = ref('')
const outputName = ref('合集_merge.mp4')
const isMerging = ref(false)
const isDragOver = ref(false)
const dragDepth = ref(0)
const progress = ref(0)
const error = ref('')
const resultPath = ref('')
const dropzoneEl = ref<HTMLElement | null>(null)
const runningInTauri = isTauri()
let unlistenDragDrop: UnlistenFn | undefined
let lastDropKey = ''
let lastDropAt = 0

const canMerge = computed(
  () =>
    items.value.length >= 2 &&
    Boolean(outputPath.value) &&
    Boolean(outputName.value.trim()) &&
    !isMerging.value,
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

function appendVideos(videos: PickedVideo[]) {
  if (!videos.length) return
  const existing = new Set(items.value.map((i) => i.path))
  const next = videos
    .filter((v) => !existing.has(v.path))
    .map((v) => ({ ...v, id: makeId() }))
  items.value = [...items.value, ...next]
}

async function chooseVideos() {
  if (isMerging.value || !runningInTauri) return
  appendVideos(await pickVideoFiles())
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

function onFileChange(e: Event) {
  const input = e.target as HTMLInputElement
  if (input.files) addBrowserFiles(input.files)
  input.value = ''
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

function removeItem(id: string) {
  if (isMerging.value) return
  items.value = items.value.filter((i) => i.id !== id)
}

function clearAll() {
  if (isMerging.value) return
  items.value = []
}

function move(id: string, dir: -1 | 1) {
  if (isMerging.value) return
  const index = items.value.findIndex((i) => i.id === id)
  const target = index + dir
  if (index < 0 || target < 0 || target >= items.value.length) return
  const copy = [...items.value]
  const [row] = copy.splice(index, 1)
  copy.splice(target, 0, row)
  items.value = copy
}

async function pickOutputDir() {
  const selected = await pickOutputDirectory(outputPath.value || undefined)
  if (selected) outputPath.value = selected
}

async function startMerge() {
  if (!canMerge.value) return
  if (!runningInTauri) {
    window.alert('请使用桌面应用进行合成')
    return
  }
  isMerging.value = true
  progress.value = 0
  error.value = ''
  resultPath.value = ''

  const outName = outputName.value.trim()
  const inputPaths = items.value.map((i) => i.path)
  const outputDir = outputPath.value
  const taskId = await createTask({
    type: 'merge',
    title: outName,
    meta: { outputDir, videoCount: inputPaths.length },
  })
  activeMergeTaskId.value = taskId
  let aborted = false
  registerAbortHandler(taskId, () => {
    aborted = true
  })

  items.value = []
  goToPage('tasks')

  try {
    await prepareCompressBatch()
    const result = await mergeVideos({
      id: makeId(),
      inputPaths,
      outputDir,
      outputName: outName,
      onProgress: (p) => {
        progress.value = p
        updateTaskProgress(taskId, p)
      },
    })
    progress.value = 100
    resultPath.value = result.outputPath
    if (tasks.value.find((t) => t.id === taskId)?.status === 'cancelled' || aborted) {
      // already cancelled
    } else {
      completeTask(taskId, { outputPath: result.outputPath })
    }
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e)
    error.value = message
    const current = tasks.value.find((t) => t.id === taskId)
    if (current?.status === 'cancelled' || aborted) {
      if (current?.status !== 'cancelled') cancelTaskLocal(taskId)
    } else {
      failTask(taskId, message)
    }
  } finally {
    unregisterAbortHandler(taskId)
    if (activeMergeTaskId.value === taskId) activeMergeTaskId.value = null
    isMerging.value = false
  }
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
    if (isMerging.value) return

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
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1>视频合成</h1>
        <p>按顺序将多个视频合并为一个文件</p>
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
          :disabled="isMerging"
          @change="onFileChange"
        />
        <h2>拖拽或选择多个视频</h2>
        <p>支持 mp4 / mov / mkv · 至少 2 个才能合成</p>
        <button type="button" class="btn btn-ghost" tabindex="-1" @click.stop="chooseVideos">
          选择文件
        </button>
      </div>

      <div v-if="items.length" class="list-section selection-section">
        <div class="list-head">
          <h3>已选视频</h3>
          <div class="list-head-actions">
            <span>{{ items.length }} 个片段 · 按顺序合成</span>
            <button
              type="button"
              class="btn btn-ghost btn-sm"
              :disabled="isMerging"
              @click="clearAll"
            >
              清空
            </button>
          </div>
        </div>
        <ul class="file-list selected-list">
          <li v-for="(item, index) in items" :key="item.id" class="file-item merge-item">
            <span class="index">{{ String(index + 1).padStart(2, '0') }}</span>
            <div class="file-meta">
              <p class="file-name" :title="item.name">{{ item.name }}</p>
              <p class="file-path" :title="item.path">{{ item.path }}</p>
              <p class="file-sub">{{ formatSize(item.size) }}</p>
            </div>
            <div class="file-actions">
              <button
                type="button"
                class="btn btn-ghost btn-sm"
                :disabled="isMerging || index === 0"
                @click="move(item.id, -1)"
              >
                上移
              </button>
              <button
                type="button"
                class="btn btn-ghost btn-sm"
                :disabled="isMerging || index === items.length - 1"
                @click="move(item.id, 1)"
              >
                下移
              </button>
              <button
                type="button"
                class="btn btn-ghost btn-sm"
                :disabled="isMerging"
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
              {{ outputPath || '选择合并后的保存目录' }}
            </p>
          </div>
          <button type="button" class="btn btn-ghost" :disabled="isMerging" @click="pickOutputDir">
            选择路径
          </button>
        </div>

        <label class="name-field">
          <span class="output-label">输出文件名</span>
          <input v-model="outputName" class="text-input" :disabled="isMerging" />
        </label>

        <div class="action-row">
          <p class="hint">按上方列表顺序拼接；进度在「任务列表」查看</p>
          <button type="button" class="btn btn-primary" :disabled="!canMerge" @click="startMerge">
            {{ isMerging ? `合并中 ${progress}%` : '开始合并' }}
          </button>
        </div>

        <p v-if="resultPath" class="summary ok">已保存：{{ resultPath }}</p>
        <p v-if="error" class="summary err">{{ error }}</p>
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
.merge-item {
  grid-template-columns: auto 1fr auto;
}
.index {
  font-family: Sora, sans-serif;
  color: var(--accent);
  font-weight: 600;
  font-size: 0.9rem;
}
.name-field {
  display: grid;
  gap: 6px;
}
.text-input {
  width: 100%;
  padding: 11px 14px;
  border-radius: 12px;
  border: 1px solid var(--line);
  background: rgba(8, 16, 15, 0.55);
  color: var(--text);
  outline: none;
}
.text-input:focus {
  border-color: var(--accent);
}
.ok {
  color: var(--ok);
}
.err {
  color: var(--danger);
}
</style>
