<script setup lang="ts">
import { getCurrentWebview } from '@tauri-apps/api/webview'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { computed, onActivated, onDeactivated, onMounted, ref, watch } from 'vue'
import { getCompressConcurrency, compressVideo, prepareCompressBatch } from '../compress'
import {
  isTauri,
  pickOutputDirectory,
  pickVideoFiles,
  videosFromPaths,
  type PickedVideo,
} from '../desktop'
import { mergeVideos, probeVideoDimensions } from '../merge'
import { showConfirm, showDialog } from '../dialog'
import { goToPage } from '../navigation'
import { showToast } from '../toast'
import { qualityPresetLabel, settings } from '../settings'
import {
  activeBatchTaskId,
  activeMergeTaskId,
  cancelTaskLocal,
  completeTask,
  createTask,
  enqueueTaskRun,
  failTask,
  hasRunningTask,
  registerAbortHandler,
  tasks,
  unregisterAbortHandler,
  updateTaskProgress,
} from '../taskStore'
import { formatSize, makeId, toBatchName, isVideoFileName } from '../utils'

type WorkMode = 'batch' | 'merge'

type WorkItem = {
  id: string
  name: string
  size: number
  path: string
  width?: number
  height?: number
}

const VIDEO_ACCEPT = 'video/*,.mp4,.mov,.mkv,.avi,.webm,.m4v,.wmv,.flv'
const OUTPUT_DIR_KEY = 'kuaiya-workbench-output-dir'

const mode = ref<WorkMode>('batch')
const items = ref<WorkItem[]>([])
const outputPath = ref(localStorage.getItem(OUTPUT_DIR_KEY) || '')
const outputName = ref('合集_merge.mp4')
const isBusy = ref(false)
const isDragOver = ref(false)
const dragDepth = ref(0)
const dragItemId = ref<string | null>(null)
const dropTargetId = ref<string | null>(null)
const dropzoneEl = ref<HTMLElement | null>(null)
const runningInTauri = isTauri()
let unlistenDragDrop: UnlistenFn | undefined
let lastDropKey = ''
let lastDropAt = 0

const resolutionMismatch = computed(() => {
  if (mode.value !== 'merge') return false
  const sized = items.value.filter((i) => i.width && i.height)
  if (sized.length < 2) return false
  const w0 = sized[0].width!
  const h0 = sized[0].height!
  return sized.some((i) => i.width !== w0 || i.height !== h0)
})

const targetResolutionLabel = computed(() => {
  const first = items.value.find((i) => i.width && i.height)
  if (!first?.width || !first.height) return ''
  return `${first.width}×${first.height}`
})

const canSubmit = computed(() => {
  if (isBusy.value || !outputPath.value || !items.value.length) return false
  if (mode.value === 'batch') return true
  return items.value.length >= 2 && Boolean(outputName.value.trim())
})

const primaryLabel = computed(() => {
  if (isBusy.value) return '处理中…'
  if (mode.value === 'merge' && resolutionMismatch.value) return '统一分辨率并开始'
  return mode.value === 'batch' ? '开始压制' : '开始合成'
})

const dropHint = computed(() =>
  mode.value === 'batch'
    ? '每个文件单独压制，分辨率不变'
    : '按列表顺序合成一个文件 · 至少 2 个',
)

watch(outputPath, (v) => {
  if (v) localStorage.setItem(OUTPUT_DIR_KEY, v)
})

watch(mode, async (next) => {
  if (next === 'merge') await ensureDimensions(items.value)
})

async function ensureDimensions(list: WorkItem[]) {
  await Promise.all(
    list.map(async (item) => {
      if (item.width && item.height) return
      const dim = await probeVideoDimensions(item.path)
      if (dim) {
        item.width = dim.width
        item.height = dim.height
      }
    }),
  )
  items.value = [...items.value]
}

function isPointInDropzone(position: { x: number; y: number }) {
  const el = dropzoneEl.value
  if (!el) return false
  const rect = el.getBoundingClientRect()
  const scale = window.devicePixelRatio || 1
  const x = position.x / scale
  const y = position.y / scale
  return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom
}

async function appendVideos(videos: PickedVideo[]) {
  if (!videos.length) return
  const existing = new Set(items.value.map((i) => i.path || `${i.name}-${i.size}`))
  const next: WorkItem[] = videos
    .filter((v) => !existing.has(v.path || `${v.name}-${v.size}`))
    .map((v) => ({
      id: makeId(),
      name: v.name,
      size: v.size,
      path: v.path,
    }))
  if (mode.value === 'merge') await ensureDimensions(next)
  items.value = [...items.value, ...next]
}

async function chooseVideos() {
  if (isBusy.value || !runningInTauri) return
  await appendVideos(await pickVideoFiles())
}

async function addBrowserFiles(fileList: FileList | File[]) {
  const videos = Array.from(fileList).filter(
    (f) => f.type.startsWith('video/') || isVideoFileName(f.name),
  )
  await appendVideos(
    videos.map((file) => ({
      name: file.name,
      path: `browser://${file.name}-${file.size}-${file.lastModified}`,
      size: file.size,
    })),
  )
}

function onFileChange(e: Event) {
  const input = e.target as HTMLInputElement
  if (input.files) void addBrowserFiles(input.files)
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
  if (e.dataTransfer?.files?.length) void addBrowserFiles(e.dataTransfer.files)
}

function removeItem(id: string) {
  if (isBusy.value) return
  items.value = items.value.filter((i) => i.id !== id)
}

function clearAll() {
  if (isBusy.value) return
  items.value = []
}

function move(id: string, dir: -1 | 1) {
  if (isBusy.value || mode.value !== 'merge') return
  const index = items.value.findIndex((i) => i.id === id)
  const target = index + dir
  if (index < 0 || target < 0 || target >= items.value.length) return
  const copy = [...items.value]
  const [row] = copy.splice(index, 1)
  copy.splice(target, 0, row)
  items.value = copy
}

function reorderTo(fromId: string, toId: string) {
  if (fromId === toId) return
  const from = items.value.findIndex((i) => i.id === fromId)
  const to = items.value.findIndex((i) => i.id === toId)
  if (from < 0 || to < 0) return
  const copy = [...items.value]
  const [row] = copy.splice(from, 1)
  copy.splice(to, 0, row)
  items.value = copy
}

function onItemDragStart(e: DragEvent, id: string) {
  if (isBusy.value || mode.value !== 'merge') {
    e.preventDefault()
    return
  }
  dragItemId.value = id
  e.dataTransfer?.setData('text/kuaiya-item', id)
  if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
}

function onItemDragOver(e: DragEvent, id: string) {
  if (!dragItemId.value || dragItemId.value === id) return
  e.preventDefault()
  e.stopPropagation()
  dropTargetId.value = id
  if (e.dataTransfer) e.dataTransfer.dropEffect = 'move'
}

function onItemDrop(e: DragEvent, id: string) {
  e.preventDefault()
  e.stopPropagation()
  const fromId = dragItemId.value
  dragItemId.value = null
  dropTargetId.value = null
  if (fromId) reorderTo(fromId, id)
}

function onItemDragEnd() {
  dragItemId.value = null
  dropTargetId.value = null
}

async function pickOutputDir() {
  const selected = await pickOutputDirectory(outputPath.value || undefined)
  if (selected) outputPath.value = selected
}

async function startBatch() {
  isBusy.value = true
  const queue = items.value.map((item) => ({
    ...item,
    outputName: toBatchName(item.name),
  }))
  const outputDir = outputPath.value
  let aborted = false
  const willWait = hasRunningTask()

  const taskId = await createTask({
    type: 'batch',
    title: '批量压制',
    meta: { videoCount: queue.length, outputDir },
  })
  registerAbortHandler(taskId, () => {
    aborted = true
  })

  const submitted = queue.length
  items.value = []
  goToPage('tasks')
  showToast({
    message: willWait
      ? `已加入排队 · ${submitted} 个视频`
      : `已提交 · ${submitted} 个视频`,
    tone: 'ok',
    actionLabel: '查看',
    onAction: () => goToPage('tasks'),
  })
  isBusy.value = false

  enqueueTaskRun(taskId, async () => {
    if (tasks.value.find((t) => t.id === taskId)?.status === 'cancelled' || aborted) return

    activeBatchTaskId.value = taskId
    let cursor = 0
    let doneCount = 0
    let failCount = 0
    const itemProgress = new Map<string, number>()

    try {
      await prepareCompressBatch(taskId)

      function refreshTaskProgress() {
        if (aborted) return
        const partial = [...itemProgress.values()].reduce((a, b) => a + b, 0)
        const overall = ((doneCount + partial) / queue.length) * 100
        updateTaskProgress(taskId, Math.min(99, overall), {
          doneCount,
          videoCount: queue.length,
        })
      }

      async function worker() {
        while (!aborted) {
          const index = cursor
          cursor += 1
          if (index >= queue.length) break
          const item = queue[index]
          itemProgress.set(item.id, 0)
          try {
            await compressVideo({
              id: item.id,
              cancelKey: taskId,
              inputPath: item.path,
              outputDir,
              outputName: item.outputName,
              onProgress: (progress) => {
                itemProgress.set(item.id, progress / 100)
                refreshTaskProgress()
              },
            })
            if (aborted) break
            itemProgress.delete(item.id)
            doneCount += 1
            refreshTaskProgress()
          } catch {
            itemProgress.delete(item.id)
            if (aborted) break
            failCount += 1
          }
        }
      }

      const pool = Math.min(getCompressConcurrency(), queue.length)
      await Promise.all(Array.from({ length: pool }, () => worker()))

      const current = tasks.value.find((t) => t.id === taskId)
      if (current?.status === 'cancelled') {
        // already cancelled
      } else if (aborted) {
        cancelTaskLocal(taskId, '主动取消')
      } else if (doneCount >= queue.length) {
        completeTask(taskId, { outputDir, doneCount, videoCount: queue.length })
      } else {
        updateTaskProgress(taskId, Math.round((doneCount / Math.max(1, queue.length)) * 100), {
          doneCount,
          videoCount: queue.length,
        })
        failTask(taskId, failCount > 0 ? '部分文件压制失败' : '任务未完成')
      }
    } finally {
      unregisterAbortHandler(taskId)
      if (activeBatchTaskId.value === taskId) activeBatchTaskId.value = null
    }
  })
}

async function startMerge(normalizeResolution: boolean) {
  if (normalizeResolution) {
    const ok = await showConfirm({
      title: '统一分辨率',
      tone: 'warn',
      message: `所选视频分辨率不同，无法直接合成。\n是否全部统一为第一段的 ${targetResolutionLabel.value} 后合成？`,
      confirmText: '开始合成',
      cancelText: '取消',
    })
    if (!ok) return
  }

  isBusy.value = true
  const outName = outputName.value.trim()
  const inputPaths = items.value.map((i) => i.path)
  const outputDir = outputPath.value
  let aborted = false
  const willWait = hasRunningTask()
  const taskId = await createTask({
    type: 'merge',
    title: outName,
    meta: { outputDir, videoCount: inputPaths.length },
  })
  registerAbortHandler(taskId, () => {
    aborted = true
  })

  const submitted = inputPaths.length
  items.value = []
  goToPage('tasks')
  showToast({
    message: willWait
      ? `已加入排队 · ${submitted} 个片段`
      : `已提交合成 · ${submitted} 个片段`,
    tone: 'ok',
    actionLabel: '查看',
    onAction: () => goToPage('tasks'),
  })
  isBusy.value = false

  enqueueTaskRun(taskId, async () => {
    if (tasks.value.find((t) => t.id === taskId)?.status === 'cancelled' || aborted) return

    activeMergeTaskId.value = taskId
    try {
      await prepareCompressBatch(taskId)
      const result = await mergeVideos({
        id: makeId(),
        cancelKey: taskId,
        inputPaths,
        outputDir,
        outputName: outName,
        normalizeResolution,
        onProgress: (p) => updateTaskProgress(taskId, p),
      })
      if (tasks.value.find((t) => t.id === taskId)?.status === 'cancelled' || aborted) {
        // already cancelled
      } else {
        completeTask(taskId, {
          outputPath: result.outputPath,
          doneCount: inputPaths.length,
          videoCount: inputPaths.length,
        })
      }
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e)
      const current = tasks.value.find((t) => t.id === taskId)
      if (current?.status === 'cancelled' || aborted) {
        if (current?.status !== 'cancelled') cancelTaskLocal(taskId, '主动取消')
      } else {
        failTask(taskId, message || '合成失败')
      }
    } finally {
      unregisterAbortHandler(taskId)
      if (activeMergeTaskId.value === taskId) activeMergeTaskId.value = null
    }
  })
}

async function onConfirm() {
  if (!canSubmit.value) return
  if (!runningInTauri) {
    await showDialog({
      title: '提示',
      tone: 'warn',
      message: '请使用桌面应用完成压制或合成',
    })
    return
  }
  if (mode.value === 'batch') {
    await startBatch()
    return
  }
  await startMerge(resolutionMismatch.value)
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
    if (isBusy.value) return

    const dropKey = payload.paths.join('\0')
    const now = Date.now()
    if (dropKey === lastDropKey && now - lastDropAt < 400) return
    lastDropKey = dropKey
    lastDropAt = now

    await appendVideos(await videosFromPaths(payload.paths))
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
        <h1>压制合成</h1>
        <p>选视频、选输出目录，一次完成压制或合成</p>
      </div>
      <button
        type="button"
        class="preset-pill"
        title="前往系统设置"
        @click="goToPage('settings')"
      >
        {{ qualityPresetLabel(settings.qualityPreset) }}
      </button>
    </header>

    <section class="panel">
      <div class="mode-bar" role="group" aria-label="处理方式">
        <button
          type="button"
          class="mode-option"
          :class="{ active: mode === 'batch' }"
          :disabled="isBusy"
          @click="mode = 'batch'"
        >
          <span class="mode-title">分别压制</span>
          <span class="mode-desc">每个视频单独输出</span>
        </button>
        <button
          type="button"
          class="mode-option"
          :class="{ active: mode === 'merge' }"
          :disabled="isBusy"
          @click="mode = 'merge'"
        >
          <span class="mode-title">合并成片</span>
          <span class="mode-desc">按顺序合成一个文件</span>
        </button>
      </div>

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
          :disabled="isBusy"
          @change="onFileChange"
        />
        <h2>拖拽或选择多个视频</h2>
        <p>{{ dropHint }}</p>
        <button type="button" class="btn btn-soft" tabindex="-1" @click.stop="chooseVideos">
          选择文件
        </button>
      </div>

      <div v-if="items.length" class="list-section selection-section">
        <div class="list-head">
          <h3>已选视频</h3>
          <div class="list-head-actions">
            <span>
              {{ items.length }} 个
              <template v-if="mode === 'merge'"> · 拖拽排序 · 按顺序合成</template>
              <template v-if="resolutionMismatch"> · 分辨率不一致</template>
            </span>
            <button
              type="button"
              class="btn btn-text btn-sm"
              :disabled="isBusy"
              @click="clearAll"
            >
              清空
            </button>
          </div>
        </div>
        <ul class="file-list selected-list">
          <li
            v-for="(item, index) in items"
            :key="item.id"
            class="file-item"
            :class="{
              'merge-item': mode === 'merge',
              'is-dragging': dragItemId === item.id,
              'is-drop-target': dropTargetId === item.id,
            }"
            :draggable="mode === 'merge' && !isBusy"
            @dragstart="onItemDragStart($event, item.id)"
            @dragover="onItemDragOver($event, item.id)"
            @drop="onItemDrop($event, item.id)"
            @dragend="onItemDragEnd"
          >
            <span v-if="mode === 'merge'" class="drag-handle" aria-hidden="true" title="拖拽排序">
              <svg viewBox="0 0 12 16" fill="currentColor">
                <circle cx="4" cy="3" r="1.2" />
                <circle cx="8" cy="3" r="1.2" />
                <circle cx="4" cy="8" r="1.2" />
                <circle cx="8" cy="8" r="1.2" />
                <circle cx="4" cy="13" r="1.2" />
                <circle cx="8" cy="13" r="1.2" />
              </svg>
            </span>
            <span v-if="mode === 'merge'" class="index">{{
              String(index + 1).padStart(2, '0')
            }}</span>
            <div class="file-meta">
              <p class="file-name" :title="item.name">{{ item.name }}</p>
              <p class="file-path" :title="item.path">{{ item.path }}</p>
              <p class="file-sub">
                {{ formatSize(item.size) }}
                <template v-if="mode === 'batch'"> · 输出 {{ toBatchName(item.name) }}</template>
                <template v-else-if="item.width && item.height">
                  · {{ item.width }}×{{ item.height }}
                </template>
              </p>
            </div>
            <div class="file-actions">
              <template v-if="mode === 'merge'">
                <button
                  type="button"
                  class="btn btn-text btn-sm"
                  :disabled="isBusy || index === 0"
                  @click="move(item.id, -1)"
                >
                  上移
                </button>
                <button
                  type="button"
                  class="btn btn-text btn-sm"
                  :disabled="isBusy || index === items.length - 1"
                  @click="move(item.id, 1)"
                >
                  下移
                </button>
              </template>
              <button
                type="button"
                class="btn btn-text btn-sm"
                :disabled="isBusy"
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
              {{ outputPath || '选择保存目录（两种模式共用）' }}
            </p>
          </div>
          <button type="button" class="btn btn-soft" :disabled="isBusy" @click="pickOutputDir">
            选择路径
          </button>
        </div>

        <Transition name="fade-slide">
          <label v-if="mode === 'merge'" class="name-field">
            <span class="output-label">合成文件名</span>
            <input v-model="outputName" class="text-input" :disabled="isBusy" />
          </label>
        </Transition>

        <p v-if="resolutionMismatch" class="summary err mismatch-tip">
          分辨率不同，无法直接合成。开始后将统一为第一段的 {{ targetResolutionLabel }}。
        </p>

        <div class="action-row">
          <p class="hint">
            <template v-if="mode === 'batch'">
              并行 <code>{{ getCompressConcurrency() }}</code> 个 · 输出名加
              <code>_batch</code>
            </template>
            <template v-else>按上方顺序拼接</template>
            · 开始后前往任务列表
          </p>
          <button
            type="button"
            class="btn btn-primary"
            :disabled="!canSubmit"
            @click="onConfirm"
          >
            {{ primaryLabel }}
          </button>
        </div>
      </div>
    </section>
  </div>
</template>

<style scoped>
.mode-bar {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
  margin: 0 0 16px;
  padding: 6px;
  border-radius: 14px;
  border: 1px solid var(--line-strong);
  background: rgba(8, 16, 15, 0.45);
}

.mode-option {
  display: grid;
  gap: 2px;
  padding: 12px 14px;
  border: none;
  border-radius: 10px;
  background: transparent;
  color: var(--text-muted);
  text-align: left;
  transition:
    background 0.18s ease,
    color 0.18s ease,
    box-shadow 0.18s ease;
}

.mode-option:hover:not(:disabled) {
  color: var(--text);
}

.mode-option.active {
  background: var(--accent-soft);
  color: var(--accent);
  box-shadow: inset 0 0 0 1px rgba(94, 234, 212, 0.28);
}

.mode-option:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.mode-title {
  font-size: 0.95rem;
  font-weight: 700;
  letter-spacing: 0.01em;
}

.mode-desc {
  font-size: 0.78rem;
  opacity: 0.78;
  font-weight: 500;
}

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
  grid-template-columns: auto auto 1fr auto;
  cursor: grab;
}

.merge-item:active {
  cursor: grabbing;
}

.merge-item.is-dragging {
  opacity: 0.45;
}

.merge-item.is-drop-target {
  border-color: rgba(94, 234, 212, 0.45);
  background: rgba(94, 234, 212, 0.08);
}

.drag-handle {
  display: grid;
  place-items: center;
  width: 14px;
  color: var(--text-dim);
  opacity: 0.7;
}

.drag-handle svg {
  width: 10px;
  height: 14px;
  display: block;
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

.err {
  color: var(--danger);
}

.mismatch-tip {
  margin: 0;
}

.fade-slide-enter-active,
.fade-slide-leave-active {
  transition:
    opacity 0.2s ease,
    transform 0.2s ease;
}

.fade-slide-enter-from,
.fade-slide-leave-to {
  opacity: 0;
  transform: translateY(-6px);
}

@media (max-width: 640px) {
  .mode-bar {
    grid-template-columns: 1fr;
  }
}
</style>
