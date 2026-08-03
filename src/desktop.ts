import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import { stat } from '@tauri-apps/plugin-fs'

export type PickedVideo = {
  name: string
  path: string
  size: number
}

export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

/** 在访达 / 资源管理器中打开目录，或定位到文件 */
export async function revealPath(path: string): Promise<void> {
  if (!isTauri() || !path) return
  await invoke('reveal_path', { path })
}

function basename(path: string) {
  const normalized = path.replace(/\\/g, '/')
  const parts = normalized.split('/')
  return parts[parts.length - 1] || path
}

export async function pickOutputDirectory(current?: string): Promise<string | null> {
  if (isTauri()) {
    const selected = await open({
      directory: true,
      multiple: false,
      defaultPath: current || undefined,
      title: '选择输出文件夹',
    })
    return typeof selected === 'string' ? selected : null
  }

  const picker = (
    window as Window & {
      showDirectoryPicker?: () => Promise<{ name: string }>
    }
  ).showDirectoryPicker

  if (typeof picker === 'function') {
    try {
      const dir = await picker()
      return `/${dir.name}`
    } catch {
      return null
    }
  }

  const fallback = window.prompt(
    '请输入输出文件夹路径（浏览器模拟）',
    current || '~/Movies/影工输出',
  )
  return fallback && fallback.trim() ? fallback.trim() : null
}

const VIDEO_EXTENSIONS = new Set([
  'mp4',
  'mov',
  'mkv',
  'avi',
  'webm',
  'm4v',
  'wmv',
  'flv',
])

function isVideoPath(path: string) {
  const name = basename(path)
  const dot = name.lastIndexOf('.')
  if (dot < 0) return false
  return VIDEO_EXTENSIONS.has(name.slice(dot + 1).toLowerCase())
}

export async function videosFromPaths(paths: string[]): Promise<PickedVideo[]> {
  const videos: PickedVideo[] = []
  for (const path of paths) {
    if (!isVideoPath(path)) continue
    let size = 0
    try {
      const info = await stat(path)
      size = Number(info.size) || 0
    } catch {
      size = 0
    }
    videos.push({ name: basename(path), path, size })
  }
  return videos
}

export async function pickVideoFiles(): Promise<PickedVideo[]> {
  if (!isTauri()) return []

  const selected = await open({
    multiple: true,
    title: '选择视频文件',
    filters: [
      {
        name: '视频',
        extensions: [...VIDEO_EXTENSIONS],
      },
    ],
  })

  if (!selected) return []
  const paths = Array.isArray(selected) ? selected : [selected]
  return videosFromPaths(paths)
}
