import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { isTauri } from './desktop'
import type { DramaFolder } from './types'

export type DramaScanProgress = {
  dirsScanned: number
  dramasFound: number
  videosFound: number
  currentName: string
}

export async function listDramaFolders(
  watchDir: string,
  onProgress?: (progress: DramaScanProgress) => void,
): Promise<DramaFolder[]> {
  if (!isTauri()) {
    throw new Error('请使用桌面应用进行自动压制')
  }
  let unlisten: UnlistenFn | undefined
  try {
    if (onProgress) {
      unlisten = await listen<DramaScanProgress>('drama-scan-progress', (event) => {
        onProgress(event.payload)
      })
    }
    return await invoke<DramaFolder[]>('list_drama_folders', { watchDir })
  } finally {
    unlisten?.()
  }
}
