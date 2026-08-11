/// <reference types="node" />

import assert from 'node:assert/strict'
import test from 'node:test'
import {
  runAutoDramaJob,
  type AutoDramaQueueItem,
} from './autoBatch.ts'
import { publishPersistedDramaDone } from './autoPersistence.ts'

const queue: AutoDramaQueueItem[] = [
  {
    id: 'broken',
    inputPath: '/inputs/broken.mp4',
    outputDir: '/outputs/drama',
    outputName: 'broken_batch.mp4',
    outputPath: '/outputs/drama/broken_batch.mp4',
  },
  {
    id: 'later',
    inputPath: '/inputs/later.mp4',
    outputDir: '/outputs/drama',
    outputName: 'later_batch.mp4',
    outputPath: '/outputs/drama/later_batch.mp4',
  },
]

function createOperations(options: {
  validOutputs: Set<string>
  attempts: Map<string, number>
  failBrokenAttempts: number
}) {
  return {
    prepareBatch: async () => {},
    isOutputValid: async (outputPath: string) => options.validOutputs.has(outputPath),
    compress: async (item: AutoDramaQueueItem) => {
      const attempts = (options.attempts.get(item.inputPath) ?? 0) + 1
      options.attempts.set(item.inputPath, attempts)
      if (item.id === 'broken' && attempts <= options.failBrokenAttempts) {
        throw new Error(`broken attempt ${attempts}`)
      }
      options.validOutputs.add(item.outputPath)
    },
    isCancelled: () => false,
    onProgress: () => {},
  }
}

test('a failed video does not stop later videos in the drama', async () => {
  const validOutputs = new Set<string>()
  const attempts = new Map<string, number>()

  const result = await runAutoDramaJob(
    queue,
    1,
    createOperations({ validOutputs, attempts, failBrokenAttempts: 2 }),
  )

  assert.equal(attempts.get('/inputs/broken.mp4'), 2)
  assert.equal(attempts.get('/inputs/later.mp4'), 1)
  assert.deepEqual(result, {
    status: 'failed',
    message: '部分文件压制失败',
    meta: {
      outputDir: '/outputs/drama',
      videoCount: 2,
      doneCount: 1,
      completedCount: 1,
      skippedCount: 0,
      failures: [
        {
          inputPath: '/inputs/broken.mp4',
          message: 'broken attempt 2',
        },
      ],
    },
  })
})

test('a later drama run skips valid output and retries only the failed video', async () => {
  const validOutputs = new Set<string>()
  const firstAttempts = new Map<string, number>()
  await runAutoDramaJob(
    queue,
    2,
    createOperations({ validOutputs, attempts: firstAttempts, failBrokenAttempts: 2 }),
  )

  const retryAttempts = new Map<string, number>()
  const result = await runAutoDramaJob(
    queue,
    2,
    createOperations({ validOutputs, attempts: retryAttempts, failBrokenAttempts: 0 }),
  )

  assert.equal(retryAttempts.get('/inputs/broken.mp4'), 1)
  assert.equal(retryAttempts.has('/inputs/later.mp4'), false)
  assert.deepEqual(result, {
    status: 'completed',
    meta: {
      outputDir: '/outputs/drama',
      videoCount: 2,
      doneCount: 2,
      completedCount: 1,
      skippedCount: 1,
      failures: [],
    },
  })
})

test('persistence failure does not publish a drama completion record', async () => {
  const existing = [
    {
      path: '/dramas/already-done',
      name: 'Already done',
      completedAt: '2026-08-07 18:00',
      videoCount: 1,
    },
  ]
  let published = existing

  await assert.rejects(
    publishPersistedDramaDone(
      existing,
      {
        path: '/dramas/new',
        name: 'New drama',
        completedAt: '2026-08-07 18:30',
        videoCount: 2,
      },
      {
        persist: async () => {
          throw new Error('disk full')
        },
        publish: (records) => {
          published = records
        },
      },
    ),
    /disk full/,
  )

  assert.equal(published.some((record) => record.path === '/dramas/new'), false)
  assert.equal(published, existing)
})

test('debug mirror failure does not reject or suppress a persisted completion', async () => {
  const existing: Array<{
    path: string
    name: string
    completedAt: string
    videoCount: number
  }> = []
  let published = existing
  let mirrorCalls = 0

  await publishPersistedDramaDone(
    existing,
    {
      path: '/dramas/persisted',
      name: 'Persisted drama',
      completedAt: '2026-08-07 18:45',
      videoCount: 3,
    },
    {
      persist: async () => {},
      mirror: () => {
        mirrorCalls += 1
        throw new Error('localStorage quota exceeded')
      },
      publish: (records) => {
        published = records
      },
    },
  )

  assert.equal(mirrorCalls, 1)
  assert.equal(published.some((record) => record.path === '/dramas/persisted'), true)
})
