import { invoke } from '@tauri-apps/api/core'
import { ref } from 'vue'
import { cancelCompress } from './compress'
import { isTauri } from './desktop'
import { makeId } from './utils'

export type TaskType = 'batch' | 'merge' | 'auto'
export type TaskStatus = 'pending' | 'running' | 'done' | 'error' | 'cancelled'

export type AppTask = {
  id: string
  type: TaskType
  title: string
  status: TaskStatus
  progress: number
  error?: string
  meta?: string
  createdAt: string
  updatedAt: string
}

export const tasks = ref<AppTask[]>([])
export const tasksReady = ref(false)

/** 当前进行中的任务 id，供取消时定位前端 abort */
export const activeBatchTaskId = ref<string | null>(null)
export const activeMergeTaskId = ref<string | null>(null)
export const activeAutoTaskId = ref<string | null>(null)

/** 由各业务模块注册，任务列表取消时调用 */
type AbortHandler = () => void
const abortHandlers = new Map<string, AbortHandler>()

/** 全局单任务队列：同时只跑一个，其余保持等待中 */
type QueuedJob = {
  taskId: string
  run: () => Promise<void>
}
const jobQueue: QueuedJob[] = []
let currentRunningId: string | null = null
let pumping = false

let persistTimer: number | undefined
const pendingPersist = new Map<string, AppTask>()

function nowIso() {
  return new Date().toISOString()
}

function sortTasks(list: AppTask[]) {
  return [...list].sort((a, b) => {
    const bu = b.updatedAt.localeCompare(a.updatedAt)
    if (bu !== 0) return bu
    return b.createdAt.localeCompare(a.createdAt)
  })
}

function toPayload(task: AppTask) {
  return {
    id: task.id,
    type: task.type,
    title: task.title,
    status: task.status,
    progress: task.progress,
    error: task.error ?? null,
    meta: task.meta ?? null,
    createdAt: task.createdAt,
    updatedAt: task.updatedAt,
  }
}

async function persistNow(task: AppTask) {
  if (!isTauri()) return
  await invoke('upsert_task', { task: toPayload(task) })
}

function schedulePersist(task: AppTask) {
  pendingPersist.set(task.id, task)
  if (persistTimer) return
  persistTimer = window.setTimeout(async () => {
    persistTimer = undefined
    const batch = [...pendingPersist.values()]
    pendingPersist.clear()
    for (const item of batch) {
      try {
        await persistNow(item)
      } catch (e) {
        console.error('保存任务失败', e)
      }
    }
  }, 300)
}

function patchLocal(id: string, patch: Partial<AppTask>, immediate = false) {
  const idx = tasks.value.findIndex((t) => t.id === id)
  if (idx < 0) return
  const next: AppTask = {
    ...tasks.value[idx],
    ...patch,
    updatedAt: nowIso(),
  }
  const copy = [...tasks.value]
  copy[idx] = next
  tasks.value = sortTasks(copy)
  if (immediate) {
    pendingPersist.delete(id)
    void persistNow(next)
  } else {
    schedulePersist(next)
  }
}

export function registerAbortHandler(taskId: string, handler: AbortHandler) {
  abortHandlers.set(taskId, handler)
}

export function unregisterAbortHandler(taskId: string) {
  abortHandlers.delete(taskId)
}

export async function initTaskStore() {
  if (!isTauri()) {
    tasksReady.value = true
    return
  }
  try {
    const rows = await invoke<AppTask[]>('list_tasks')
    tasks.value = sortTasks(rows)
  } catch (e) {
    console.error('加载任务列表失败', e)
    tasks.value = []
  } finally {
    tasksReady.value = true
  }
}

export async function createTask(input: {
  type: TaskType
  title: string
  meta?: TaskMeta
}): Promise<string> {
  const id = makeId()
  const ts = nowIso()
  const task: AppTask = {
    id,
    type: input.type,
    title: input.title,
    status: 'pending',
    progress: 0,
    meta: input.meta ? JSON.stringify(input.meta) : undefined,
    createdAt: ts,
    updatedAt: ts,
  }
  tasks.value = sortTasks([task, ...tasks.value])
  await persistNow(task)
  return id
}

/** 是否已有正在执行或已在执行队列中的任务（不含尚未入队的新任务） */
export function hasRunningTask() {
  return (
    currentRunningId != null ||
    jobQueue.length > 0 ||
    tasks.value.some((t) => t.status === 'running')
  )
}

/** 将任务加入全局执行队列；同一时间只执行一个 */
export function enqueueTaskRun(taskId: string, run: () => Promise<void>) {
  if (jobQueue.some((j) => j.taskId === taskId)) return
  jobQueue.push({ taskId, run })
  void pumpQueue()
}

function removeQueuedJob(taskId: string) {
  const idx = jobQueue.findIndex((j) => j.taskId === taskId)
  if (idx >= 0) jobQueue.splice(idx, 1)
}

async function pumpQueue() {
  if (pumping) return
  pumping = true
  try {
    while (jobQueue.length > 0) {
      const job = jobQueue.shift()!
      const task = tasks.value.find((t) => t.id === job.taskId)
      if (!task || task.status === 'cancelled') continue

      currentRunningId = job.taskId
      patchLocal(
        job.taskId,
        {
          status: 'running',
          error: undefined,
          meta: mergeMeta(task, { startedAt: nowIso() }),
        },
        true,
      )
      try {
        await job.run()
      } catch (e) {
        const current = tasks.value.find((t) => t.id === job.taskId)
        if (current?.status === 'running') {
          failTask(job.taskId, e instanceof Error ? e.message : String(e))
        }
      } finally {
        if (currentRunningId === job.taskId) currentRunningId = null
      }
    }
  } finally {
    pumping = false
    if (jobQueue.length > 0) void pumpQueue()
  }
}

/** 取消排队中、尚未开始的指定类型任务 */
export function cancelPendingTasksByType(type: TaskType, reason: string) {
  const ids = tasks.value
    .filter((t) => t.type === type && t.status === 'pending')
    .map((t) => t.id)
  for (const id of ids) {
    removeQueuedJob(id)
    cancelTaskLocal(id, reason)
  }
}

export type TaskFailure = {
  inputPath: string
  message: string
}

export type TaskMeta = {
  videoCount?: number
  doneCount?: number
  completedCount?: number
  skippedCount?: number
  failures?: Array<TaskFailure>
  outputDir?: string
  outputPath?: string
  dramaPath?: string
  /** 真正开始执行（离开排队）的时间；耗时从此刻起算，不含等待 */
  startedAt?: string
  finishedAt?: string
  durationMs?: number
  /** 编码器使用统计，如 { VT: 72, x264: 8 } */
  encoderCounts?: Record<string, number>
  /** 每文件实际编码器 */
  encoderByFile?: Array<{ name: string; encoder: string }>
  /** 如 VT×72 · x264×8 */
  encoderSummary?: string
  [key: string]: unknown
}

export function parseTaskMeta(raw?: string): TaskMeta {
  if (!raw) return {}
  try {
    return JSON.parse(raw) as TaskMeta
  } catch {
    return {}
  }
}

export function taskFailures(task: AppTask): TaskFailure[] {
  if (task.status !== 'error') return []
  const failures = parseTaskMeta(task.meta).failures
  if (!Array.isArray(failures)) return []
  return failures.filter(
    (failure): failure is TaskFailure =>
      typeof failure?.inputPath === 'string' && typeof failure.message === 'string',
  )
}

function mergeMeta(current: AppTask | undefined, patch?: TaskMeta): string | undefined {
  if (!patch) return current?.meta
  return JSON.stringify({ ...parseTaskMeta(current?.meta), ...patch })
}

function resolveStartMs(current: AppTask | undefined): number {
  if (!current) return NaN
  const meta = parseTaskMeta(current.meta)
  const startRaw = meta.startedAt || current.createdAt
  return Date.parse(String(startRaw).replace(' ', 'T'))
}

function withDuration(current: AppTask | undefined, patch?: TaskMeta): TaskMeta {
  const finishedAt = nowIso()
  const started = resolveStartMs(current)
  const durationMs = Number.isFinite(started) ? Math.max(0, Date.parse(finishedAt) - started) : 0
  return { ...patch, finishedAt, durationMs }
}

export function updateTaskProgress(
  id: string,
  progress: number,
  counts?: { doneCount?: number; videoCount?: number },
) {
  const current = tasks.value.find((t) => t.id === id)
  const metaPatch: TaskMeta = {}
  if (counts?.doneCount != null) metaPatch.doneCount = counts.doneCount
  if (counts?.videoCount != null) metaPatch.videoCount = counts.videoCount
  patchLocal(
    id,
    {
      progress: Math.max(0, Math.min(100, Math.round(progress))),
      ...(Object.keys(metaPatch).length ? { meta: mergeMeta(current, metaPatch) } : {}),
    },
    false,
  )
}

export function completeTask(id: string, meta?: TaskMeta) {
  const current = tasks.value.find((t) => t.id === id)
  const base = parseTaskMeta(current?.meta)
  const videoCount = Number(meta?.videoCount ?? base.videoCount) || 0
  const nextMeta = mergeMeta(
    current,
    withDuration(current, {
      ...meta,
      doneCount: meta?.doneCount ?? videoCount,
      videoCount: videoCount || base.videoCount,
    }),
  )
  patchLocal(id, { status: 'done', progress: 100, error: undefined, meta: nextMeta }, true)
  clearActive(id)
  unregisterAbortHandler(id)
}

export function failTask(id: string, error: string, meta?: TaskMeta) {
  const current = tasks.value.find((t) => t.id === id)
  patchLocal(
    id,
    { status: 'error', error, meta: mergeMeta(current, withDuration(current, meta)) },
    true,
  )
  clearActive(id)
  unregisterAbortHandler(id)
}

/** 取消任务；手动取消请传「主动取消」 */
export function cancelTaskLocal(id: string, reason = '主动取消') {
  const current = tasks.value.find((t) => t.id === id)
  patchLocal(
    id,
    {
      status: 'cancelled',
      error: reason,
      meta: mergeMeta(current, withDuration(current)),
    },
    true,
  )
  clearActive(id)
  unregisterAbortHandler(id)
}

/** 失败 / 取消时展示的原因文案 */
export function taskReason(task: AppTask): string {
  if (task.status === 'error') return task.error?.trim() || '未知错误'
  if (task.status === 'cancelled') return task.error?.trim() || '主动取消'
  return task.error?.trim() || ''
}

export function resolveDurationMs(task: AppTask): number | undefined {
  const meta = parseTaskMeta(task.meta)
  if (meta.durationMs != null && Number.isFinite(Number(meta.durationMs))) {
    return Math.max(0, Number(meta.durationMs))
  }
  const endRaw = meta.finishedAt || task.updatedAt
  const start = resolveStartMs(task)
  const end = Date.parse(String(endRaw).replace(' ', 'T'))
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) return undefined
  return end - start
}

export function formatDuration(ms?: number) {
  if (ms == null || !Number.isFinite(ms) || ms < 0) return ''
  const minutes = ms / 60000
  if (minutes < 1) return '不到1分钟'
  // 整分钟直接显示；否则保留1位小数
  const rounded = Math.round(minutes * 10) / 10
  if (Number.isInteger(rounded)) return `${rounded}分钟`
  return `${rounded.toFixed(1)}分钟`
}

function clearActive(id: string) {
  if (activeBatchTaskId.value === id) activeBatchTaskId.value = null
  if (activeMergeTaskId.value === id) activeMergeTaskId.value = null
  if (activeAutoTaskId.value === id) activeAutoTaskId.value = null
}

export async function cancelTask(id: string, reason = '主动取消') {
  const task = tasks.value.find((t) => t.id === id)
  if (!task || (task.status !== 'running' && task.status !== 'pending')) return

  removeQueuedJob(id)
  abortHandlers.get(id)?.()

  // 等待中：只移出队列，不触发编码取消
  if (task.status === 'pending') {
    cancelTaskLocal(id, reason)
    return
  }

  try {
    await cancelCompress(id)
  } catch {
    // ignore
  }
  cancelTaskLocal(id, reason)
}

export async function removeTask(id: string) {
  const task = tasks.value.find((t) => t.id === id)
  if (!task) return
  if (task.status === 'running' || task.status === 'pending') {
    await cancelTask(id)
  }
  if (isTauri()) {
    await invoke('delete_task', { id })
  }
  tasks.value = tasks.value.filter((t) => t.id !== id)
  unregisterAbortHandler(id)
}

export async function clearFinishedTasks() {
  if (isTauri()) {
    await invoke('delete_finished_tasks')
  }
  tasks.value = tasks.value.filter(
    (t) => t.status === 'running' || t.status === 'pending',
  )
}

export function typeLabel(type: TaskType) {
  if (type === 'batch') return '批量压制'
  if (type === 'merge') return '视频合成'
  return '自动压制'
}

export function statusLabel(status: TaskStatus) {
  if (status === 'pending') return '等待中'
  if (status === 'running') return '进行中'
  if (status === 'done') return '已完成'
  if (status === 'cancelled') return '已取消'
  return '失败'
}
