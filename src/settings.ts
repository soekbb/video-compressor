import { invoke } from '@tauri-apps/api/core'
import { ref } from 'vue'
import { isTauri } from './desktop'
import type { AppSettings } from './types'

const STORAGE_KEY = 'kuaiya-settings'

const defaults: AppSettings = {
  concurrency: 2,
  scanIntervalMinutes: 3,
}

function clampConcurrency(n: number) {
  return Math.min(5, Math.max(1, Math.round(Number(n)) || 2))
}

function clampInterval(n: number) {
  return Math.min(60, Math.max(3, Math.round(Number(n)) || 3))
}

function normalize(partial: Partial<AppSettings>): AppSettings {
  return {
    concurrency: clampConcurrency(partial.concurrency ?? defaults.concurrency),
    scanIntervalMinutes: clampInterval(
      partial.scanIntervalMinutes ?? defaults.scanIntervalMinutes,
    ),
  }
}

function readLocalFallback(): AppSettings | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return null
    return normalize(JSON.parse(raw) as Partial<AppSettings>)
  } catch {
    return null
  }
}

export const settings = ref<AppSettings>({ ...defaults })
export const settingsReady = ref(false)

async function persist(value: AppSettings) {
  if (!isTauri()) {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(value))
    return
  }
  await invoke('save_settings', {
    settings: {
      concurrency: value.concurrency,
      scanIntervalMinutes: value.scanIntervalMinutes,
    },
  })
}

export async function initSettings() {
  if (!isTauri()) {
    settings.value = readLocalFallback() ?? { ...defaults }
    settingsReady.value = true
    return
  }

  try {
    const loaded = await invoke<AppSettings>('load_settings')
    let next = normalize(loaded)

    // 若仍是默认值，尝试迁移旧 localStorage 配置一次
    const legacy = readLocalFallback()
    if (
      legacy &&
      loaded.concurrency === defaults.concurrency &&
      loaded.scanIntervalMinutes === defaults.scanIntervalMinutes &&
      (legacy.concurrency !== defaults.concurrency ||
        legacy.scanIntervalMinutes !== defaults.scanIntervalMinutes)
    ) {
      next = legacy
      await persist(next)
      localStorage.removeItem(STORAGE_KEY)
    }

    settings.value = next
  } catch (e) {
    console.error('加载设置失败', e)
    settings.value = readLocalFallback() ?? { ...defaults }
  } finally {
    settingsReady.value = true
  }
}

export async function updateSettings(patch: Partial<AppSettings>) {
  const next = normalize({
    concurrency: patch.concurrency ?? settings.value.concurrency,
    scanIntervalMinutes: patch.scanIntervalMinutes ?? settings.value.scanIntervalMinutes,
  })
  settings.value = next
  try {
    await persist(next)
  } catch (e) {
    console.error('保存设置失败', e)
    throw e
  }
}

export function getConcurrency() {
  return settings.value.concurrency
}
