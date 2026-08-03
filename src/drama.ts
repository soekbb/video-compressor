import { invoke } from '@tauri-apps/api/core'
import { isTauri } from './desktop'
import type { DramaFolder } from './types'

export async function listDramaFolders(watchDir: string): Promise<DramaFolder[]> {
  if (!isTauri()) {
    throw new Error('请使用桌面应用进行自动压制')
  }
  return invoke<DramaFolder[]>('list_drama_folders', { watchDir })
}
