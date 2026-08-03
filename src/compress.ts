import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { isTauri } from './desktop'
import { getConcurrency, getQualityPreset } from './settings'

export type CompressResult = {
  outputPath: string
  outputSize: number
}

export type CompressProgress = {
  id: string
  progress: number
}

export function getCompressConcurrency() {
  return getConcurrency()
}

/** 开始一批编码前清除该任务的取消标记 */
export async function prepareCompressBatch(cancelKey: string) {
  if (!isTauri() || !cancelKey) return
  await invoke('prepare_compress_batch', { cancelKey })
}

/** 仅取消指定任务的编码，不影响其它任务 */
export async function cancelCompress(cancelKey: string) {
  if (!isTauri() || !cancelKey) return
  await invoke('cancel_compress', { cancelKey })
}

export async function compressVideo(params: {
  id: string
  /** 任务级取消键，通常为 AppTask.id */
  cancelKey: string
  inputPath: string
  outputDir: string
  outputName: string
  onProgress?: (progress: number) => void
}): Promise<CompressResult> {
  if (!isTauri()) {
    throw new Error('请使用桌面应用进行真实压缩（npm run tauri:dev）')
  }

  let unlisten: UnlistenFn | undefined
  try {
    unlisten = await listen<CompressProgress>('compress-progress', (event) => {
      if (event.payload.id === params.id) {
        params.onProgress?.(event.payload.progress)
      }
    })

    return await invoke<CompressResult>('compress_video', {
      id: params.id,
      inputPath: params.inputPath,
      outputDir: params.outputDir,
      outputName: params.outputName,
      qualityPreset: getQualityPreset(),
      cancelKey: params.cancelKey,
    })
  } finally {
    unlisten?.()
  }
}
