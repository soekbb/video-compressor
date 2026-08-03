<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import {
  autoDone,
  autoEnabled,
  autoWatchDir,
  clearAutoDone,
  initAutoStore,
  isDramaDone,
  markDramaDone,
} from '../autoStore'
import { getCompressConcurrency, compressVideo, prepareCompressBatch } from '../compress'
import { listDramaFolders } from '../drama'
import { isTauri, pickOutputDirectory } from '../desktop'
import { settings } from '../settings'
import {
  activeAutoTaskId,
  cancelTaskLocal,
  completeTask,
  createTask,
  failTask,
  registerAbortHandler,
  tasks,
  unregisterAbortHandler,
  updateTaskProgress,
} from '../taskStore'
import type { DramaFolder } from '../types'
import { makeId, toBatchName } from '../utils'

type JobStatus = 'queued' | 'running' | 'done' | 'error'

type DramaJob = {
  path: string
  name: string
  videoCount: number
  status: JobStatus
  progress: number
  doneCount: number
  error?: string
}

const runningInTauri = isTauri()
const folders = ref<DramaFolder[]>([])
const jobs = ref<DramaJob[]>([])
const isScanning = ref(false)
const isWorking = ref(false)
const scanError = ref('')
const nextScanIn = ref(0)
const lastScanAt = ref('')
let timer: number | undefined
let countdownTimer: number | undefined
let abort = false

const pendingFolders = computed(() => folders.value.filter((f) => !isDramaDone(f.path)))

async function pickWatchDir() {
  const selected = await pickOutputDirectory(autoWatchDir.value || undefined)
  if (selected) {
    autoWatchDir.value = selected
    await scanNow()
  }
}

async function scanNow() {
  if (!runningInTauri) {
    scanError.value = '请使用桌面应用'
    return
  }
  if (!autoWatchDir.value) {
    scanError.value = '请先选择监控目录'
    return
  }
  isScanning.value = true
  scanError.value = ''
  try {
    folders.value = await listDramaFolders(autoWatchDir.value)
    lastScanAt.value = new Date().toLocaleTimeString()
    enqueuePending()
    if (autoEnabled.value && !isWorking.value) {
      void processQueue()
    }
  } catch (e) {
    scanError.value = e instanceof Error ? e.message : String(e)
  } finally {
    isScanning.value = false
    resetCountdown()
  }
}

function enqueuePending() {
  for (const folder of pendingFolders.value) {
    const existing = jobs.value.find((j) => j.path === folder.path)
    if (existing && (existing.status === 'queued' || existing.status === 'running')) continue
    jobs.value = [
      ...jobs.value.filter((j) => j.path !== folder.path),
      {
        path: folder.path,
        name: folder.name,
        videoCount: folder.videoCount,
        status: 'queued',
        progress: 0,
        doneCount: 0,
      },
    ]
  }
}

function updateDramaProgress(
  jobPath: string,
  doneCount: number,
  total: number,
  partial = 0,
  taskId?: string,
) {
  const progress = Math.min(99, Math.round(((doneCount + partial) / total) * 100))
  jobs.value = jobs.value.map((j) =>
    j.path === jobPath ? { ...j, doneCount, progress } : j,
  )
  if (taskId) updateTaskProgress(taskId, progress)
}

async function runDrama(job: DramaJob) {
  const folder = folders.value.find((f) => f.path === job.path)
  if (!folder) {
    jobs.value = jobs.value.map((j) =>
      j.path === job.path ? { ...j, status: 'error', error: '找不到剧目文件夹' } : j,
    )
    return
  }

  jobs.value = jobs.value.map((j) =>
    j.path === job.path
      ? { ...j, status: 'running', progress: 0, doneCount: 0, error: undefined }
      : j,
  )

  const taskId = await createTask({
    type: 'auto',
    title: folder.name,
    meta: { dramaPath: folder.path, videoCount: folder.videoCount },
  })
  activeAutoTaskId.value = taskId
  registerAbortHandler(taskId, () => {
    abort = true
  })

  const sep = autoWatchDir.value.includes('\\') ? '\\' : '/'
  const outputDir = `${autoWatchDir.value}${sep}影工输出${sep}${folder.name}`
  const total = folder.videos.length
  let doneCount = 0
  let cursor = 0
  const videoProgress = new Map<string, number>()

  try {
    await prepareCompressBatch()

    async function worker() {
      while (!abort && autoEnabled.value) {
        const index = cursor
        cursor += 1
        if (index >= folder!.videos.length) break
        const video = folder!.videos[index]
        const id = makeId()
        videoProgress.set(id, 0)

        await compressVideo({
          id,
          inputPath: video.path,
          outputDir,
          outputName: toBatchName(video.name),
          onProgress: (p) => {
            videoProgress.set(id, p / 100)
            const partial = [...videoProgress.values()].reduce((a, b) => a + b, 0)
            updateDramaProgress(job.path, doneCount, total, partial, taskId)
          },
        })

        videoProgress.delete(id)
        doneCount += 1
        updateDramaProgress(job.path, doneCount, total, 0, taskId)
      }
    }

    // 同一时间只跑一个剧目；剧目内按设置并行压制多个视频
    const pool = Math.min(getCompressConcurrency(), Math.max(1, total))
    await Promise.all(Array.from({ length: pool }, () => worker()))

    const cancelled = tasks.value.find((t) => t.id === taskId)?.status === 'cancelled'
    if (abort || !autoEnabled.value || cancelled) {
      if (!cancelled) cancelTaskLocal(taskId)
      jobs.value = jobs.value.map((j) =>
        j.path === job.path ? { ...j, status: 'error', error: '已停止' } : j,
      )
      return
    }
    if (doneCount < total) throw new Error('剧目未全部完成')

    markDramaDone({
      path: folder.path,
      name: folder.name,
      completedAt: new Date().toLocaleString(),
      videoCount: folder.videoCount,
    })
    jobs.value = jobs.value.map((j) =>
      j.path === job.path ? { ...j, status: 'done', progress: 100, doneCount: total } : j,
    )
    completeTask(taskId, { outputDir })
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e)
    jobs.value = jobs.value.map((j) =>
      j.path === job.path ? { ...j, status: 'error', error: message } : j,
    )
    const current = tasks.value.find((t) => t.id === taskId)
    if (current?.status !== 'cancelled') failTask(taskId, message)
  } finally {
    unregisterAbortHandler(taskId)
    if (activeAutoTaskId.value === taskId) activeAutoTaskId.value = null
  }
}

async function processQueue() {
  if (isWorking.value || !autoEnabled.value) return
  isWorking.value = true

  // 自动压制：同一时间只执行一个剧目；取消单条后继续后续队列
  while (autoEnabled.value) {
    const next = jobs.value.find((j) => j.status === 'queued')
    if (!next) break
    abort = false
    await runDrama(next)
  }

  isWorking.value = false
}

function resetCountdown() {
  nextScanIn.value = Math.max(1, settings.value.scanIntervalMinutes) * 60
}

function startTimers() {
  stopTimers()
  resetCountdown()
  countdownTimer = window.setInterval(() => {
    if (!autoEnabled.value) return
    nextScanIn.value = Math.max(0, nextScanIn.value - 1)
  }, 1000)
  timer = window.setInterval(() => {
    if (autoEnabled.value) void scanNow()
  }, Math.max(1, settings.value.scanIntervalMinutes) * 60 * 1000)
}

function stopTimers() {
  if (timer) window.clearInterval(timer)
  if (countdownTimer) window.clearInterval(countdownTimer)
  timer = undefined
  countdownTimer = undefined
}

async function toggleEnabled() {
  autoEnabled.value = !autoEnabled.value
  if (autoEnabled.value) {
    await scanNow()
    startTimers()
    void processQueue()
  } else {
    abort = true
    stopTimers()
  }
}

function formatCountdown(sec: number) {
  const m = Math.floor(sec / 60)
  const s = sec % 60
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
}

onMounted(async () => {
  await initAutoStore()
  if (autoWatchDir.value) await scanNow()
  if (autoEnabled.value) startTimers()
})

onBeforeUnmount(() => {
  // KeepAlive 切页时不会卸载；真正销毁时也不强制中止任务
  stopTimers()
})
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1>自动压制</h1>
        <p>
          同时只压制一个剧目；剧目内按设置并行处理视频。每
          {{ settings.scanIntervalMinutes }} 分钟扫描一次
        </p>
      </div>
      <button
        type="button"
        class="btn"
        :class="autoEnabled ? 'btn-primary' : 'btn-ghost'"
        @click="toggleEnabled"
      >
        {{ autoEnabled ? '自动扫描已开启' : '启用自动扫描' }}
      </button>
    </header>

    <section class="panel">
      <div class="controls" style="border-top: none">
        <div class="output-row">
          <div class="output-field">
            <span class="output-label">监控目录</span>
            <p class="output-path" :class="{ 'is-placeholder': !autoWatchDir }">
              {{ autoWatchDir || '选择包含多个剧目文件夹的根目录' }}
            </p>
          </div>
          <button type="button" class="btn btn-ghost" @click="pickWatchDir">选择监控目录</button>
        </div>
        <div class="action-row">
          <p class="hint">
            每个子文件夹视为一个剧目。输出到
            <code>监控目录/影工输出/剧目名/</code>，并记录已完成，避免重复压制。
          </p>
          <button type="button" class="btn btn-ghost" :disabled="isScanning" @click="scanNow">
            {{ isScanning ? '扫描中…' : '立即扫描' }}
          </button>
        </div>
        <p class="summary">
          待处理 {{ pendingFolders.length }} 个剧目 · 已完成记录 {{ autoDone.length }}
          <template v-if="lastScanAt"> · 上次扫描 {{ lastScanAt }}</template>
          <template v-if="autoEnabled"> · 下次扫描 {{ formatCountdown(nextScanIn) }}</template>
        </p>
        <div class="action-row">
          <p class="hint">进度与结果请到「任务列表」查看</p>
          <button
            v-if="autoDone.length"
            type="button"
            class="btn btn-ghost btn-sm"
            @click="clearAutoDone"
          >
            清空已完成记录
          </button>
        </div>
        <p v-if="scanError" class="summary err">{{ scanError }}</p>
      </div>
    </section>
  </div>
</template>

<style scoped>
.err {
  color: var(--danger);
}
</style>
