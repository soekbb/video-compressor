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
  meta?: Record<string, unknown>
}): Promise<string> {
  const id = makeId()
  const ts = nowIso()
  const task: AppTask = {
    id,
    type: input.type,
    title: input.title,
    status: 'running',
    progress: 0,
    meta: input.meta ? JSON.stringify(input.meta) : undefined,
    createdAt: ts,
    updatedAt: ts,
  }
  tasks.value = sortTasks([task, ...tasks.value])
  await persistNow(task)
  return id
}

export function updateTaskProgress(id: string, progress: number) {
  patchLocal(id, { progress: Math.max(0, Math.min(100, Math.round(progress))) }, false)
}

function parseMeta(raw?: string): Record<string, unknown> {
  if (!raw) return {}
  try {
    return JSON.parse(raw) as Record<string, unknown>
  } catch {
    return {}
  }
}

export function completeTask(id: string, meta?: Record<string, unknown>) {
  const current = tasks.value.find((t) => t.id === id)
  const nextMeta =
    meta != null ? JSON.stringify({ ...parseMeta(current?.meta), ...meta }) : current?.meta
  patchLocal(id, { status: 'done', progress: 100, error: undefined, meta: nextMeta }, true)
  clearActive(id)
  unregisterAbortHandler(id)
}

export function failTask(id: string, error: string) {
  patchLocal(id, { status: 'error', error }, true)
  clearActive(id)
  unregisterAbortHandler(id)
}

export function cancelTaskLocal(id: string) {
  patchLocal(id, { status: 'cancelled', error: '已取消' }, true)
  clearActive(id)
  unregisterAbortHandler(id)
}

function clearActive(id: string) {
  if (activeBatchTaskId.value === id) activeBatchTaskId.value = null
  if (activeMergeTaskId.value === id) activeMergeTaskId.value = null
  if (activeAutoTaskId.value === id) activeAutoTaskId.value = null
}

export async function cancelTask(id: string) {
  const task = tasks.value.find((t) => t.id === id)
  if (!task || (task.status !== 'running' && task.status !== 'pending')) return

  abortHandlers.get(id)?.()
  try {
    await cancelCompress()
  } catch {
    // ignore
  }
  cancelTaskLocal(id)
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
