/// <reference types="node" />

import assert from 'node:assert/strict'
import test from 'node:test'
import { runManualBatchJob, type ManualBatchQueueItem } from './manualBatch.ts'

const firstItem: ManualBatchQueueItem = {
  id: 'first',
  inputPath: '/inputs/clip.mp4',
  outputDir: '/outputs',
  outputName: 'clip_batch.mp4',
  outputPath: '/outputs/clip_batch.mp4',
}

function createOperations(overrides: Record<string, unknown> = {}) {
  const invokes: string[] = []
  const progress: number[] = []
  const completions: unknown[] = []
  const failures: Array<{ message: string; meta: unknown }> = []
  let cancellations = 0

  return {
    invokes,
    progress,
    completions,
    failures,
    get cancellations() {
      return cancellations
    },
    operations: {
      prepareBatch: async () => {
        invokes.push('prepare')
      },
      isOutputValid: async () => {
        invokes.push('validate')
        return false
      },
      compress: async () => {
        invokes.push('compress')
      },
      isCancelled: () => false,
      onProgress: (value: number) => {
        progress.push(value)
      },
      onComplete: (meta: unknown) => {
        completions.push(meta)
      },
      onFail: (message: string, meta: unknown) => {
        failures.push({ message, meta })
      },
      onCancelled: () => {
        cancellations += 1
      },
      ...overrides,
    },
  }
}

test('collision preflight aborts before any Tauri invoke', async () => {
  const harness = createOperations()
  const duplicate = {
    ...firstItem,
    id: 'second',
    inputPath: '/other/clip.mp4',
  }

  await assert.rejects(
    runManualBatchJob([firstItem, duplicate], 2, harness.operations),
    /输出路径冲突/,
  )
  assert.deepEqual(harness.invokes, [])
})

test('validated output skips compression and persists skipped count', async () => {
  const harness = createOperations({
    isOutputValid: async () => {
      harness.invokes.push('validate')
      return true
    },
  })

  await runManualBatchJob([firstItem], 1, harness.operations)

  assert.deepEqual(harness.invokes, ['prepare', 'validate'])
  assert.deepEqual(harness.completions, [
    {
      outputDir: '/outputs',
      videoCount: 1,
      doneCount: 1,
      completedCount: 0,
      skippedCount: 1,
      failures: [],
    },
  ])
})

test('initial compression failure invokes exactly one retry', async () => {
  let attempts = 0
  const harness = createOperations({
    compress: async () => {
      harness.invokes.push('compress')
      attempts += 1
      if (attempts === 1) throw new Error('first failure')
    },
  })

  await runManualBatchJob([firstItem], 1, harness.operations)

  assert.equal(attempts, 2)
  assert.deepEqual(harness.invokes, ['prepare', 'validate', 'compress', 'compress'])
})

test('final failure persists final error metadata without resetting processed progress', async () => {
  let attempts = 0
  const harness = createOperations({
    compress: async () => {
      attempts += 1
      throw new Error(attempts === 1 ? 'first failure' : 'final ffmpeg failure')
    },
  })

  await runManualBatchJob([firstItem], 1, harness.operations)

  assert.equal(attempts, 2)
  assert.deepEqual(harness.failures, [
    {
      message: '部分文件压制失败',
      meta: {
        outputDir: '/outputs',
        videoCount: 1,
        doneCount: 0,
        completedCount: 0,
        skippedCount: 0,
        failures: [
          {
            inputPath: '/inputs/clip.mp4',
            message: 'final ffmpeg failure',
          },
        ],
      },
    },
  ])
  assert.equal(harness.progress.at(-1), 99)
})

test('cancellation neither retries nor reports progress after cancellation', async () => {
  let cancelled = false
  let attempts = 0
  let emitProgress: ((value: number) => void) | undefined
  const harness = createOperations({
    isCancelled: () => cancelled,
    compress: async (_item: ManualBatchQueueItem, onProgress: (value: number) => void) => {
      attempts += 1
      emitProgress = onProgress
      onProgress(80)
      cancelled = true
      onProgress(100)
      throw new Error('cancelled')
    },
  })

  await runManualBatchJob([firstItem], 1, harness.operations)
  emitProgress?.(50)

  assert.equal(attempts, 1)
  assert.deepEqual(harness.progress, [80])
  assert.equal(harness.cancellations, 1)
  assert.deepEqual(harness.completions, [])
  assert.deepEqual(harness.failures, [])
})

test('onItemDone fires for each completed item before cancel returns', async () => {
  const second: ManualBatchQueueItem = {
    id: 'second',
    inputPath: '/inputs/two.mp4',
    outputDir: '/outputs',
    outputName: 'two_batch.mp4',
    outputPath: '/outputs/two_batch.mp4',
  }
  const doneNames: string[] = []
  let cancelled = false
  const harness = createOperations({
    isCancelled: () => cancelled,
    compress: async (item: ManualBatchQueueItem) => {
      if (item.id === 'first') return
      cancelled = true
      throw new Error('cancelled')
    },
    onItemDone: async (item: ManualBatchQueueItem, outcome: string) => {
      doneNames.push(`${item.id}:${outcome}`)
    },
  })

  await runManualBatchJob([firstItem, second], 1, harness.operations)

  assert.deepEqual(doneNames, ['first:completed'])
  assert.equal(harness.cancellations, 1)
})
