import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { isTauri } from './desktop'
import { getConcurrency } from './settings'

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

export async function prepareCompressBatch() {
  if (!isTauri()) return
  await invoke('prepare_compress_batch')
}

export async function cancelCompress() {
  if (!isTauri()) return
  await invoke('cancel_compress')
}

export async function compressVideo(params: {
  id: string
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
    })
  } finally {
    unlisten?.()
  }
}
