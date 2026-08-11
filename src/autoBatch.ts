import {
  runManualBatchJob,
  type ManualBatchMeta,
  type ManualBatchQueueItem,
} from './manualBatch.ts'

export type AutoDramaQueueItem = ManualBatchQueueItem

export type AutoDramaJobResult =
  | { status: 'completed'; meta: ManualBatchMeta }
  | { status: 'failed'; message: string; meta: ManualBatchMeta }
  | { status: 'cancelled' }

type AutoDramaOperations = {
  prepareBatch: () => Promise<unknown>
  isOutputValid: (outputPath: string) => Promise<boolean>
  compress: (
    item: AutoDramaQueueItem,
    onProgress: (progress: number) => void,
  ) => Promise<unknown>
  isCancelled: () => boolean
  onProgress: (progress: number, counts: { doneCount: number; videoCount: number }) => void
}

export async function runAutoDramaJob(
  queue: AutoDramaQueueItem[],
  concurrency: number,
  operations: AutoDramaOperations,
): Promise<AutoDramaJobResult> {
  let result: AutoDramaJobResult | undefined

  await runManualBatchJob(queue, concurrency, {
    ...operations,
    onComplete: (meta) => {
      result = { status: 'completed', meta }
    },
    onFail: (message, meta) => {
      result = { status: 'failed', message, meta }
    },
    onCancelled: () => {
      result = { status: 'cancelled' }
    },
  })

  if (!result) throw new Error('自动压制任务未返回结果')
  return result
}
