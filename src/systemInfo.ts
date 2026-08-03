import { invoke } from '@tauri-apps/api/core'
import { isTauri } from './desktop'
import type { SystemInfo } from './types'

export function recommendConcurrency(cores: number): number {
  const n = Math.max(1, Math.round(Number(cores) || 1))
  if (n <= 4) return 1
  if (n <= 8) return 2
  if (n <= 12) return 3
  if (n <= 16) return 4
  return 5
}

export function formatMemory(bytes?: number | null): string | null {
  if (bytes == null || !Number.isFinite(bytes) || bytes <= 0) return null
  const gb = bytes / (1024 * 1024 * 1024)
  if (gb >= 10) return `${Math.round(gb)} GB`
  return `${gb.toFixed(1).replace(/\.0$/, '')} GB`
}

function browserFallback(): SystemInfo {
  const cores = typeof navigator !== 'undefined' ? navigator.hardwareConcurrency || 1 : 1
  let os = '浏览器'
  if (typeof navigator !== 'undefined') {
    const ua = navigator.userAgent
    if (/Mac/i.test(ua)) os = 'macOS'
    else if (/Windows/i.test(ua)) os = 'Windows'
    else if (/Linux/i.test(ua)) os = 'Linux'
  }
  return {
    os,
    arch: 'unknown',
    cpuBrand: null,
    cpuCores: cores,
    totalMemoryBytes: null,
  }
}

export async function loadSystemInfo(): Promise<SystemInfo> {
  if (!isTauri()) return browserFallback()
  try {
    return await invoke<SystemInfo>('get_system_info')
  } catch (e) {
    console.error('读取本机信息失败', e)
    return browserFallback()
  }
}
