<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { seedVideoNames, videosToProcess } from '../autoScan'
import {
  appendDramaVideoSuccess,
  autoDone,
  autoEnabled,
  autoWatchDir,
  clearAutoDone,
  initAutoStore,
  recordDramaVideoFailure,
  seedDramaVideoNames,
  setAutoWatchDir,
} from '../autoStore'
import { runAutoDramaJob, type AutoDramaQueueItem } from '../autoBatch'
import { getCompressConcurrency, compressVideo, prepareCompressBatch } from '../compress'
import { listDramaFolders } from '../drama'
import { isTauri, pickOutputDirectory } from '../desktop'
import { showDialog } from '../dialog'
import { initSettings, settings } from '../settings'
import {
  activeAutoTaskId,
  cancelPendingTasksByType,
  cancelTask,
  cancelTaskLocal,
  completeTask,
  createTask,
  enqueueTaskRun,
  failTask,
  parseTaskMeta,
  registerAbortHandler,
  tasks,
  unregisterAbortHandler,
  updateTaskProgress,
} from '../taskStore'
import type { DramaFolder } from '../types'
import { makeId, toOutputPath } from '../utils'

type JobStatus = 'queued' | 'running' | 'done' | 'error'

type DramaJob = {
  path: string
  name: string
  videoCount: number
  videos: DramaFolder['videos']
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
/** 立即扫描触发的一次处理，不依赖「自动扫描已开启」 */
const manualBurst = ref(false)
let timer: number | undefined
let countdownTimer: number | undefined

function recordFor(path: string) {
  return autoDone.value.find((r) => r.path === path)
}

function videosForFolder(folder: DramaFolder) {
  return videosToProcess(folder, recordFor(folder.path))
}

function isBusyLocallyOrInTasks(path: string) {
  const local = jobs.value.find((j) => j.path === path)
  if (local && (local.status === 'queued' || local.status === 'running')) {
    return true
  }

  return tasks.value.some((t) => {
    if (t.type !== 'auto') return false
    if (parseTaskMeta(t.meta).dramaPath !== path) return false
    return t.status === 'pending' || t.status === 'running'
  })
}

/** 已排队/进行中，或无可压视频（含超龄）时跳过；缺 videoNames 的种子轮不跳过 */
function shouldSkipDrama(path: string) {
  const folder = folders.value.find((f) => f.path === path)
  if (!folder) return true
  if (isBusyLocallyOrInTasks(path)) return true
  const pending = videosForFolder(folder)
  if (pending === null) return false
  return pending.length === 0
}

const pendingFolders = computed(() => folders.value.filter((f) => !shouldSkipDrama(f.path)))

const compressPendingFolders = computed(() =>
  folders.value.filter((f) => {
    if (isBusyLocallyOrInTasks(f.path)) return false
    const pending = videosForFolder(f)
    return pending !== null && pending.length > 0
  }),
)

const queueJobs = computed(() =>
  jobs.value.filter((j) => j.status === 'queued' || j.status === 'running' || j.status === 'error'),
)

const activeJob = computed(() => jobs.value.find((j) => j.status === 'running'))

function jobStatusLabel(status: JobStatus) {
  if (status === 'queued') return '排队中'
  if (status === 'running') return '压制中'
  if (status === 'done') return '已完成'
  return '失败'
}

function canProcess() {
  return autoEnabled.value || manualBurst.value
}

async function refreshFolderList() {
  if (!runningInTauri || !autoWatchDir.value) return
  isScanning.value = true
  scanError.value = ''
  try {
    folders.value = await listDramaFolders(autoWatchDir.value)
    lastScanAt.value = new Date().toLocaleTimeString()
  } catch (e) {
    scanError.value = e instanceof Error ? e.message : String(e)
  } finally {
    isScanning.value = false
  }
}

async function pickWatchDir() {
  const selected = await pickOutputDirectory(autoWatchDir.value || undefined)
  if (selected) {
    await setAutoWatchDir(selected)
    // 未开启自动扫描时只刷新列表，不入队、不压制
    if (autoEnabled.value) await scanNow()
    else await refreshFolderList()
  }
}

async function scanNow(options?: { notify?: boolean }) {
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
    // 入队前统计待压剧目（入队后会标为 busy，不能再靠 compressPendingFolders）
    const compressPending = compressPendingFolders.value.map((f) => ({
      name: f.name,
      pendingCount: videosForFolder(f)?.length ?? 0,
    }))
    // 仅「自动扫描已开启」或「立即扫描」才入队并处理
    const shouldProcess = autoEnabled.value || Boolean(options?.notify)
    if (shouldProcess) {
      await enqueuePending()
      if (compressPending.length > 0 || jobs.value.some((j) => j.status === 'queued')) {
        if (options?.notify) manualBurst.value = true
        if (!isWorking.value) void processQueue()
      }
    }
    if (options?.notify) {
      const notifyCount = compressPending.length
      if (notifyCount > 0) {
        const preview = compressPending.slice(0, 6)
        await showDialog({
          title: '扫描完成',
          tone: 'ok',
          message: `发现 ${notifyCount} 个待处理剧目，已开始压制`,
          items: preview.map((f) => ({
            title: f.name,
            meta: `${f.pendingCount} 个视频`,
          })),
          itemsMore: Math.max(0, notifyCount - preview.length),
        })
      } else if (folders.value.length > 0) {
        await showDialog({
          title: '扫描完成',
          tone: 'info',
          message: '未找到新剧目\n进行中或已完成的已自动跳过',
        })
      } else {
        await showDialog({
          title: '扫描完成',
          tone: 'warn',
          message: '未找到新文件夹',
        })
      }
    }
  } catch (e) {
    scanError.value = e instanceof Error ? e.message : String(e)
    if (options?.notify) {
      await showDialog({
        title: '扫描失败',
        tone: 'danger',
        message: scanError.value,
      })
    }
  } finally {
    isScanning.value = false
    resetCountdown()
  }
}

async function enqueuePending() {
  const at = new Date().toLocaleString()
  for (const folder of pendingFolders.value) {
    if (shouldSkipDrama(folder.path)) continue
    const pending = videosForFolder(folder)
    if (pending === null) {
      await seedDramaVideoNames({
        path: folder.path,
        name: folder.name,
        videoNames: seedVideoNames(folder),
        at,
      })
      continue
    }
    if (pending.length === 0) continue

    const existing = jobs.value.find((j) => j.path === folder.path)
    if (existing && (existing.status === 'queued' || existing.status === 'running')) {
      continue
    }
    jobs.value = [
      ...jobs.value.filter((j) => j.path !== folder.path),
      {
        path: folder.path,
        name: folder.name,
        videoCount: pending.length,
        videos: pending,
        status: 'queued',
        progress: 0,
        doneCount: 0,
      },
    ]
  }
}

/** 创建自动压制任务并加入全局单任务队列（不立刻执行编码） */
async function submitDrama(job: DramaJob) {
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

  let aborted = false
  const taskId = await createTask({
    type: 'auto',
    title: folder.name,
    meta: { dramaPath: folder.path, videoCount: job.videoCount },
  })
  registerAbortHandler(taskId, () => {
    aborted = true
  })

  const outputDir = folder.path
  const videos = job.videos
  const total = videos.length

  enqueueTaskRun(taskId, async () => {
    if (tasks.value.find((t) => t.id === taskId)?.status === 'cancelled' || aborted) {
      jobs.value = jobs.value.map((j) =>
        j.path === job.path
          ? { ...j, status: 'error', error: '主动取消' }
          : j,
      )
      return
    }

    activeAutoTaskId.value = taskId

    try {
      const queue: AutoDramaQueueItem[] = videos.map((video) => {
        const outputName = video.name
        return {
          id: makeId(),
          inputPath: video.path,
          outputDir,
          outputName,
          outputPath: toOutputPath(outputDir, outputName),
        }
      })
      const result = await runAutoDramaJob(queue, getCompressConcurrency(), {
        prepareBatch: () => prepareCompressBatch(taskId),
        // 原位替换：源文件本身不能用 ffprobe「已有有效输出」跳过
        isOutputValid: async () => false,
        isCancelled: () => aborted || tasks.value.some((t) => t.id === taskId && t.status === 'cancelled'),
        compress: (item, onProgress) =>
          compressVideo({
            id: item.id,
            cancelKey: taskId,
            inputPath: item.inputPath,
            outputDir: item.outputDir,
            outputName: item.outputName,
            onProgress,
          }),
        // 每成功一个就落盘，取消时不会丢掉已原位替换的文件
        onItemDone: async (item) => {
          await appendDramaVideoSuccess({
            path: folder.path,
            name: folder.name,
            videoName: item.outputName,
            at: new Date().toLocaleString(),
          })
        },
        onProgress: (progress, counts) => {
          jobs.value = jobs.value.map((j) =>
            j.path === job.path
              ? { ...j, doneCount: counts.doneCount, progress: Math.round(progress) }
              : j,
          )
          updateTaskProgress(taskId, progress, counts)
        },
      })

      const currentTask = tasks.value.find((t) => t.id === taskId)
      const cancelled = currentTask?.status === 'cancelled'
      if (result.status === 'cancelled' || aborted || cancelled) {
        const reason = currentTask?.error || '主动取消'
        if (!cancelled) cancelTaskLocal(taskId, reason)
        jobs.value = jobs.value.map((j) =>
          j.path === job.path ? { ...j, status: 'error', error: reason } : j,
        )
        return
      }

      // 成功已在 onItemDone 写入；失败统一在成功之后记录，避免无 prev 时被丢弃
      const at = new Date().toLocaleString()
      for (const failure of result.meta.failures) {
        const item = queue.find((q) => q.inputPath === failure.inputPath)
        await recordDramaVideoFailure({
          path: folder.path,
          name: folder.name,
          videoName: item?.outputName ?? failure.inputPath.split(/[/\\]/).pop() ?? failure.inputPath,
          reason: failure.message,
          at,
        })
      }

      if (result.status === 'failed') {
        const failureCount = result.meta.failures.length
        const failMessage = `${result.message}（${failureCount} 个文件）`
        jobs.value = jobs.value.map((j) =>
          j.path === job.path
            ? {
                ...j,
                status: 'error',
                progress: Math.min(99, j.progress),
                doneCount: result.meta.doneCount,
                error: failMessage,
              }
            : j,
        )
        failTask(taskId, failMessage, result.meta)
        return
      }

      jobs.value = jobs.value.map((j) =>
        j.path === job.path ? { ...j, status: 'done', progress: 100, doneCount: total } : j,
      )
      completeTask(taskId, result.meta)
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e)
      const current = tasks.value.find((t) => t.id === taskId)
      if (current?.status === 'cancelled' || aborted || message.includes('已取消')) {
        const reason = current?.error || (aborted ? '主动取消' : '任务已中断')
        if (current?.status !== 'cancelled') cancelTaskLocal(taskId, reason)
        jobs.value = jobs.value.map((j) =>
          j.path === job.path ? { ...j, status: 'error', error: reason } : j,
        )
        return
      }
      const failMessage = message.trim() || '压制失败'
      jobs.value = jobs.value.map((j) =>
        j.path === job.path ? { ...j, status: 'error', error: failMessage } : j,
      )
      failTask(taskId, failMessage)
    } finally {
      unregisterAbortHandler(taskId)
      if (activeAutoTaskId.value === taskId) activeAutoTaskId.value = null
    }
  })
}

async function processQueue() {
  if (isWorking.value || !canProcess()) return
  isWorking.value = true

  try {
    // 只负责把剧目提交进全局任务队列；真正执行由单任务泵串行
    while (canProcess()) {
      const next = jobs.value.find((j) => j.status === 'queued')
      if (!next) break
      await submitDrama(next)
    }
  } finally {
    isWorking.value = false
    manualBurst.value = false
  }
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
  if (!autoEnabled.value) {
    if (!autoWatchDir.value) {
      await showDialog({
        title: '请先选择监控目录',
        tone: 'warn',
        message: '启用自动扫描前，请先选择要监听的剧目根目录。',
      })
      return
    }
    autoEnabled.value = true
    await scanNow()
    startTimers()
    void processQueue()
    return
  }

  autoEnabled.value = false
  manualBurst.value = false
  stopTimers()
  const pendingDramaPaths = new Set(
    tasks.value
      .filter((t) => t.type === 'auto' && t.status === 'pending')
      .map((t) => String(parseTaskMeta(t.meta).dramaPath || '')),
  )
  cancelPendingTasksByType('auto', '自动扫描已关闭，任务已取消')
  if (activeAutoTaskId.value) {
    await cancelTask(activeAutoTaskId.value, '自动扫描已关闭，当前压制已停止')
  }
  jobs.value = jobs.value.map((j) => {
    if (j.status === 'queued' || (j.status === 'running' && pendingDramaPaths.has(j.path))) {
      return { ...j, status: 'error', error: '自动扫描已关闭，任务已取消' }
    }
    return j
  })
}

function formatCountdown(sec: number) {
  const m = Math.floor(sec / 60)
  const s = sec % 60
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
}

onMounted(async () => {
  await Promise.all([initAutoStore(), initSettings()])
  // 未选择监控目录时不允许保持自动扫描开启
  if (!autoWatchDir.value && autoEnabled.value) {
    autoEnabled.value = false
  }
  // 「启动时自动扫描」开启且已有监控目录时，才自动启用
  if (settings.value.autoScanOnLaunch && autoWatchDir.value) {
    autoEnabled.value = true
  }
  // 只有开启了自动扫描才在启动时扫描；否则不主动扫目录
  if (autoEnabled.value && autoWatchDir.value) {
    await scanNow()
    startTimers()
    void processQueue()
  }
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
          需先选择监控目录，再启用自动扫描。同时只压制一个剧目；每
          {{ settings.scanIntervalMinutes }} 分钟扫描一次
        </p>
      </div>
      <button
        type="button"
        class="btn"
        :class="autoEnabled ? 'btn-soft is-on' : 'btn-primary'"
        :disabled="!autoEnabled && !autoWatchDir"
        :title="!autoEnabled && !autoWatchDir ? '请先选择监控目录' : undefined"
        @click="toggleEnabled"
      >
        {{ autoEnabled ? '自动扫描已开启' : '启用自动扫描' }}
      </button>
    </header>

    <section class="panel">
      <div class="controls" style="border-top: none">
        <div class="output-row">
          <div class="output-field">
            <span class="output-label">监控目录（必选）</span>
            <p class="output-path" :class="{ 'is-placeholder': !autoWatchDir }">
              {{ autoWatchDir || '请先选择包含多个剧目文件夹的根目录' }}
            </p>
          </div>
          <button type="button" class="btn btn-soft" @click="pickWatchDir">选择监控目录</button>
        </div>
        <div class="action-row">
          <p class="hint">
            每个子文件夹视为一个剧目。压制成功后原位替换同名文件；已记录剧目在目录创建 2
            天内会增量压制新增视频。
          </p>
          <button
            type="button"
            class="btn btn-soft"
            :disabled="isScanning || !autoWatchDir"
            :title="!autoWatchDir ? '请先选择监控目录' : undefined"
            @click="scanNow({ notify: true })"
          >
            {{ isScanning ? '扫描中…' : '立即扫描' }}
          </button>
        </div>
        <p class="summary">
          待处理 {{ compressPendingFolders.length }} 个剧目 · 已完成记录 {{ autoDone.length }}
          <template v-if="lastScanAt"> · 上次扫描 {{ lastScanAt }}</template>
          <template v-if="autoEnabled"> · 下次扫描 {{ formatCountdown(nextScanIn) }}</template>
        </p>
        <div class="action-row">
          <p class="hint">
            <template v-if="activeJob">
              正在处理「{{ activeJob.name }}」· {{ activeJob.progress }}%
            </template>
            <template v-else>详细记录也可在「任务列表」查看</template>
          </p>
          <button
            v-if="autoDone.length"
            type="button"
            class="btn btn-text btn-sm"
            @click="clearAutoDone"
          >
            清空已完成记录
          </button>
        </div>
        <ul v-if="autoDone.length" class="file-list done-list">
          <li v-for="r in autoDone" :key="r.path" class="file-item">
            <div class="file-meta">
              <p class="file-name">{{ r.name }}</p>
              <p class="file-sub">已成功 {{ r.videoNames?.length ?? r.videoCount }} 个</p>
              <details v-if="(r.videoNames?.length || r.failures?.length)">
                <summary>明细</summary>
                <p v-for="n in r.videoNames || []" :key="n">{{ n }}</p>
                <p v-for="f in r.failures || []" :key="f.name + f.at" class="err">
                  {{ f.name }}：{{ f.reason }}
                </p>
              </details>
            </div>
          </li>
        </ul>
        <p v-if="scanError" class="summary err">{{ scanError }}</p>
      </div>

      <div class="list-section queue-section">
        <div class="list-head">
          <h3>当前队列</h3>
          <span>{{ queueJobs.length }} 个</span>
        </div>
        <p v-if="!queueJobs.length" class="empty-hint">
          暂无排队或进行中的剧目。扫描到新文件夹后会显示在这里。
        </p>
        <ul v-else class="file-list auto-queue">
          <li v-for="job in queueJobs" :key="job.path" class="file-item">
            <div class="file-meta">
              <p class="file-name" :title="job.name">{{ job.name }}</p>
              <p class="file-sub">
                {{ job.doneCount }} / {{ job.videoCount }} 个视频
                <template v-if="job.error"> · {{ job.error }}</template>
              </p>
            </div>
            <div class="file-actions">
              <span
                class="status"
                :class="{
                  'status-pending': job.status === 'queued',
                  'status-running': job.status === 'running',
                  'status-error': job.status === 'error',
                  'status-done': job.status === 'done',
                }"
              >
                {{ jobStatusLabel(job.status) }}
                <template v-if="job.status === 'running'"> {{ job.progress }}%</template>
              </span>
            </div>
            <div
              v-if="job.status === 'running'"
              class="progress"
              role="progressbar"
              :aria-valuenow="job.progress"
              aria-valuemin="0"
              aria-valuemax="100"
            >
              <i :style="{ width: `${job.progress}%` }" />
            </div>
          </li>
        </ul>
      </div>
    </section>
  </div>
</template>

<style scoped>
.err {
  color: var(--danger);
}

.queue-section {
  border-top: 1px solid var(--line);
}

.auto-queue {
  max-height: 320px;
}

.done-list {
  margin-top: 0.75rem;
  max-height: 220px;
}

.done-list details {
  margin-top: 0.35rem;
  font-size: 0.85rem;
  color: var(--muted, inherit);
}

.done-list details p {
  margin: 0.15rem 0;
}
</style>
