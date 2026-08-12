import { assertUniqueOutputPaths, runBatchCompressionItem } from './utils.ts'

export type ManualBatchQueueItem = {
  id: string
  inputPath: string
  outputDir: string
  outputName: string
  outputPath: string
}

type ManualBatchFailure = {
  inputPath: string
  message: string
}

export type ManualBatchMeta = {
  outputDir: string
  videoCount: number
  doneCount: number
  completedCount: number
  skippedCount: number
  failures: ManualBatchFailure[]
}

type ManualBatchOperations = {
  prepareBatch: () => Promise<unknown>
  isOutputValid: (outputPath: string) => Promise<boolean>
  compress: (
    item: ManualBatchQueueItem,
    onProgress: (progress: number) => void,
  ) => Promise<unknown>
  isCancelled: () => boolean
  onProgress: (progress: number, counts: { doneCount: number; videoCount: number }) => void
  /** Fired after each item finishes successfully (compress or skip); awaited before next work / cancel. */
  onItemDone?: (
    item: ManualBatchQueueItem,
    outcome: 'completed' | 'skipped',
  ) => void | Promise<void>
  onComplete: (meta: ManualBatchMeta) => void
  onFail: (message: string, meta: ManualBatchMeta) => void
  onCancelled: () => void
}

export async function runManualBatchJob(
  queue: ManualBatchQueueItem[],
  concurrency: number,
  operations: ManualBatchOperations,
) {
  assertUniqueOutputPaths(queue.map((item) => item.outputPath))
  await operations.prepareBatch()

  let cursor = 0
  let completedCount = 0
  let skippedCount = 0
  const failures: ManualBatchFailure[] = []
  const itemProgress = new Map<string, number>()

  function refreshProgress() {
    if (operations.isCancelled()) return
    const partial = [...itemProgress.values()].reduce((sum, progress) => sum + progress, 0)
    const processedCount = completedCount + skippedCount + failures.length
    const overall = ((processedCount + partial) / queue.length) * 100
    operations.onProgress(Math.min(99, overall), {
      doneCount: completedCount + skippedCount,
      videoCount: queue.length,
    })
  }

  async function worker() {
    while (!operations.isCancelled()) {
      const index = cursor
      cursor += 1
      if (index >= queue.length) break

      const item = queue[index]
      itemProgress.set(item.id, 0)
      const result = await runBatchCompressionItem(
        { inputPath: item.inputPath, outputPath: item.outputPath },
        {
          isOutputValid: operations.isOutputValid,
          isCancelled: operations.isCancelled,
          compress: () =>
            operations.compress(item, (progress) => {
              if (operations.isCancelled()) return
              itemProgress.set(item.id, progress / 100)
              refreshProgress()
            }),
        },
      )
      itemProgress.delete(item.id)

      if (result.status === 'cancelled') break
      if (result.status === 'completed') {
        completedCount += 1
        await operations.onItemDone?.(item, 'completed')
      } else if (result.status === 'skipped') {
        skippedCount += 1
        await operations.onItemDone?.(item, 'skipped')
      } else {
        failures.push(result.failure)
      }
      refreshProgress()
    }
  }

  const pool = Math.min(Math.max(1, concurrency), queue.length)
  await Promise.all(Array.from({ length: pool }, () => worker()))

  if (operations.isCancelled()) {
    operations.onCancelled()
    return
  }

  const doneCount = completedCount + skippedCount
  const meta: ManualBatchMeta = {
    outputDir: queue[0]?.outputDir ?? '',
    videoCount: queue.length,
    doneCount,
    completedCount,
    skippedCount,
    failures,
  }

  if (failures.length === 0 && doneCount >= queue.length) {
    operations.onComplete(meta)
    return
  }

  refreshProgress()
  operations.onFail(failures.length > 0 ? '部分文件压制失败' : '任务未完成', meta)
}
