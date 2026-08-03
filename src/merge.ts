import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { isTauri } from './desktop'
import { getQualityPreset } from './settings'

export type MergeResult = {
  outputPath: string
  outputSize: number
}

export type VideoDimensions = {
  width: number
  height: number
}

export async function probeVideoDimensions(path: string): Promise<VideoDimensions | null> {
  if (!isTauri() || path.startsWith('browser://')) return null
  try {
    return await invoke<VideoDimensions>('probe_video_dimensions', { path })
  } catch {
    return null
  }
}

export async function mergeVideos(params: {
  id: string
  /** 任务级取消键，通常为 AppTask.id */
  cancelKey: string
  inputPaths: string[]
  outputDir: string
  outputName: string
  normalizeResolution?: boolean
  onProgress?: (progress: number) => void
}): Promise<MergeResult> {
  if (!isTauri()) {
    throw new Error('请使用桌面应用进行视频合成')
  }

  let unlisten: UnlistenFn | undefined
  try {
    unlisten = await listen<{ id: string; progress: number }>('merge-progress', (event) => {
      if (event.payload.id === params.id) {
        params.onProgress?.(event.payload.progress)
      }
    })

    return await invoke<MergeResult>('merge_videos', {
      id: params.id,
      inputPaths: params.inputPaths,
      outputDir: params.outputDir,
      outputName: params.outputName,
      qualityPreset: getQualityPreset(),
      normalizeResolution: params.normalizeResolution ?? false,
      cancelKey: params.cancelKey,
    })
  } finally {
    unlisten?.()
  }
}
