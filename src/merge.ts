import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { isTauri } from './desktop'

export type MergeResult = {
  outputPath: string
  outputSize: number
}

export async function mergeVideos(params: {
  id: string
  inputPaths: string[]
  outputDir: string
  outputName: string
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
    })
  } finally {
    unlisten?.()
  }
}
