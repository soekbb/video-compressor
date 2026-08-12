import { invoke } from '@tauri-apps/api/core'
import { nextTick, ref, watch } from 'vue'
import { createAsyncMutex } from './asyncMutex'
import {
  mergeAppendDramaVideoSuccess,
  publishPersistedDramaDone,
  publishUpsertDramaRecord,
} from './autoPersistence'
import { isTauri } from './desktop'
import type { AutoRecord } from './types'

/** Serialize done-list RMW so concurrent compress workers cannot drop sibling names. */
const runAutoDoneWrite = createAsyncMutex()

const DONE_KEY = 'kuaiya-auto-done'
const WATCH_KEY = 'kuaiya-auto-watch'
const ENABLED_KEY = 'kuaiya-auto-enabled'

type PersistState = {
  watchDir: string
  enabled: boolean
  done: AutoRecord[]
}

export const autoWatchDir = ref('')
export const autoEnabled = ref(false)
export const autoDone = ref<AutoRecord[]>([])
export const autoStoreReady = ref(false)

let persistTimer: number | undefined
let hydrating = false

function readLegacyLocal(): PersistState {
  try {
    const doneRaw = localStorage.getItem(DONE_KEY)
    return {
      watchDir: localStorage.getItem(WATCH_KEY) || '',
      enabled: localStorage.getItem(ENABLED_KEY) === '1',
      done: doneRaw ? (JSON.parse(doneRaw) as AutoRecord[]) : [],
    }
  } catch {
    return { watchDir: '', enabled: false, done: [] }
  }
}

function applyState(state: PersistState) {
  hydrating = true
  autoWatchDir.value = state.watchDir || ''
  autoEnabled.value = Boolean(state.enabled)
  autoDone.value = Array.isArray(state.done) ? state.done : []
  hydrating = false
}

async function persistCanonicalAutoState(payload: PersistState) {
  if (!isTauri() || !autoStoreReady.value || hydrating) return
  await invoke('save_auto_state', { state: payload })
}

function mirrorAutoState(payload: PersistState) {
  // 同步一份到 localStorage，便于网页调试
  localStorage.setItem(WATCH_KEY, payload.watchDir)
  localStorage.setItem(ENABLED_KEY, payload.enabled ? '1' : '0')
  localStorage.setItem(DONE_KEY, JSON.stringify(payload.done))
}

export async function persistAutoStoreNow() {
  const payload = {
    watchDir: autoWatchDir.value,
    enabled: autoEnabled.value,
    done: autoDone.value,
  }
  await persistCanonicalAutoState(payload)
  try {
    mirrorAutoState(payload)
  } catch (err) {
    console.error('同步自动压制调试记录失败', err)
  }
}

function schedulePersist() {
  if (hydrating || !autoStoreReady.value) return
  if (persistTimer) window.clearTimeout(persistTimer)
  persistTimer = window.setTimeout(() => {
    void persistAutoStoreNow().catch((err) => {
      console.error('保存自动压制记录失败', err)
    })
  }, 200)
}

/** 监控目录变更后立即落盘，避免防抖期间退出导致丢失 */
export async function setAutoWatchDir(dir: string) {
  autoWatchDir.value = dir
  if (persistTimer) {
    window.clearTimeout(persistTimer)
    persistTimer = undefined
  }
  try {
    await persistAutoStoreNow()
  } catch (err) {
    console.error('保存监控目录失败', err)
    throw err
  }
}

export async function initAutoStore() {
  if (autoStoreReady.value) return

  if (!isTauri()) {
    applyState(readLegacyLocal())
    autoStoreReady.value = true
    return
  }

  try {
    const state = await invoke<PersistState>('load_auto_state')
    const hasFileData =
      Boolean(state.watchDir) || Boolean(state.enabled) || (state.done?.length ?? 0) > 0

    if (hasFileData) {
      applyState({
        watchDir: state.watchDir || '',
        enabled: Boolean(state.enabled),
        done: state.done || [],
      })
    } else {
      // 首次迁移：把旧 localStorage 写入本地文件
      const legacy = readLegacyLocal()
      applyState(legacy)
      autoStoreReady.value = true
      await persistAutoStoreNow()
      return
    }
  } catch (err) {
    console.error('读取自动压制记录失败，回退 localStorage', err)
    applyState(readLegacyLocal())
  }

  autoStoreReady.value = true
}

watch(autoWatchDir, schedulePersist)
watch(autoEnabled, schedulePersist)
watch(autoDone, schedulePersist, { deep: true })

export function isDramaDone(path: string) {
  return autoDone.value.some((r) => r.path === path)
}

async function finishDoneWrite() {
  await nextTick()
  if (persistTimer) {
    window.clearTimeout(persistTimer)
    persistTimer = undefined
  }
}

async function upsertAutoRecordNow(record: AutoRecord) {
  await publishUpsertDramaRecord(autoDone.value, record, {
    persist: (done) =>
      persistCanonicalAutoState({
        watchDir: autoWatchDir.value,
        enabled: autoEnabled.value,
        done,
      }),
    mirror: (done) =>
      mirrorAutoState({
        watchDir: autoWatchDir.value,
        enabled: autoEnabled.value,
        done,
      }),
    onMirrorError: (err) => {
      console.error('同步自动压制调试记录失败', err)
    },
    publish: (done) => {
      autoDone.value = done
    },
  })
  await finishDoneWrite()
}

export async function markDramaDone(record: AutoRecord) {
  await runAutoDoneWrite(async () => {
    await publishPersistedDramaDone(autoDone.value, record, {
      persist: (done) =>
        persistCanonicalAutoState({
          watchDir: autoWatchDir.value,
          enabled: autoEnabled.value,
          done,
        }),
      mirror: (done) =>
        mirrorAutoState({
          watchDir: autoWatchDir.value,
          enabled: autoEnabled.value,
          done,
        }),
      onMirrorError: (err) => {
        console.error('同步自动压制调试记录失败', err)
      },
      publish: (done) => {
        autoDone.value = done
      },
    })
    await finishDoneWrite()
  })
}

export async function upsertAutoRecord(record: AutoRecord) {
  await runAutoDoneWrite(() => upsertAutoRecordNow(record))
}

/** Merge successful names into existing record (or create). Serialized against other done writes. */
export async function appendDramaVideoSuccess(args: {
  path: string
  name: string
  videoName: string
  at: string
}) {
  await runAutoDoneWrite(async () => {
    const next = mergeAppendDramaVideoSuccess(autoDone.value, args)
    const row = next.find((r) => r.path === args.path)
    if (!row) return
    await upsertAutoRecordNow(row)
  })
}

export async function recordDramaVideoFailure(args: {
  path: string
  name: string
  videoName: string
  reason: string
  at: string
}) {
  await runAutoDoneWrite(async () => {
    const prev = autoDone.value.find((r) => r.path === args.path)
    if (!prev?.videoNames?.length) {
      // Spec: never create done-only-from-failures for brand-new dramas.
      // Still update failures if record already exists.
      if (!prev) return
    }
    const failures = [
      ...(prev.failures ?? []).filter((f) => f.name !== args.videoName),
      { name: args.videoName, reason: args.reason, at: args.at },
    ]
    await upsertAutoRecordNow({
      ...prev,
      failures,
    })
  })
}

export async function seedDramaVideoNames(args: {
  path: string
  name: string
  videoNames: string[]
  at: string
}) {
  await runAutoDoneWrite(() =>
    upsertAutoRecordNow({
      path: args.path,
      name: args.name,
      completedAt: args.at,
      videoCount: args.videoNames.length,
      videoNames: args.videoNames,
      failures: [],
    }),
  )
}

export function clearAutoDone() {
  autoDone.value = []
}

export function removeAutoDone(path: string) {
  autoDone.value = autoDone.value.filter((r) => r.path !== path)
}
