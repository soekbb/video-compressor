import { invoke } from '@tauri-apps/api/core'
import { ref } from 'vue'
import { isTauri } from './desktop'
import type { AppSettings, QualityPreset } from './types'

const STORAGE_KEY = 'kuaiya-settings'

const defaults: AppSettings = {
  concurrency: 2,
  scanIntervalMinutes: 3,
  qualityPreset: 'size',
  autoScanOnLaunch: false,
}

function clampConcurrency(n: number) {
  return Math.min(5, Math.max(1, Math.round(Number(n)) || 1))
}

function clampInterval(n: number) {
  return Math.min(60, Math.max(3, Math.round(Number(n)) || 3))
}

function normalizePreset(v: unknown): QualityPreset {
  return v === 'quality' ? 'quality' : 'size'
}

function normalize(partial: Partial<AppSettings>): AppSettings {
  return {
    concurrency: clampConcurrency(partial.concurrency ?? defaults.concurrency),
    scanIntervalMinutes: clampInterval(
      partial.scanIntervalMinutes ?? defaults.scanIntervalMinutes,
    ),
    qualityPreset: normalizePreset(partial.qualityPreset ?? defaults.qualityPreset),
    autoScanOnLaunch: Boolean(partial.autoScanOnLaunch ?? defaults.autoScanOnLaunch),
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
      qualityPreset: value.qualityPreset,
      autoScanOnLaunch: value.autoScanOnLaunch,
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
    settings.value = normalize(loaded)
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
    qualityPreset: patch.qualityPreset ?? settings.value.qualityPreset,
    autoScanOnLaunch: patch.autoScanOnLaunch ?? settings.value.autoScanOnLaunch,
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

export function getQualityPreset(): QualityPreset {
  return settings.value.qualityPreset
}

export function qualityPresetLabel(preset: QualityPreset = settings.value.qualityPreset) {
  return preset === 'quality' ? '画质优先' : '体积优先'
}
