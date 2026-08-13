/// <reference types="node" />

import assert from 'node:assert/strict'
import test from 'node:test'
import {
  assertUniqueOutputPaths,
  runBatchCompressionItem,
  type BatchCompressionItem,
} from './utils.ts'

const item: BatchCompressionItem = {
  inputPath: '/inputs/clip.mp4',
  outputPath: '/outputs/clip_batch.mp4',
}

test('valid output skips compression', async () => {
  let compressCalls = 0

  const result = await runBatchCompressionItem(item, {
    isOutputValid: async () => true,
    compress: async () => {
      compressCalls += 1
    },
    isCancelled: () => false,
  })

  assert.deepEqual(result, { status: 'skipped' })
  assert.equal(compressCalls, 0)
})

test('initial compression failure retries exactly once', async () => {
  let compressCalls = 0

  const result = await runBatchCompressionItem(item, {
    isOutputValid: async () => false,
    compress: async () => {
      compressCalls += 1
      if (compressCalls === 1) throw new Error('first failure')
    },
    isCancelled: () => false,
  })

  assert.deepEqual(result, { status: 'completed', encoder: undefined })
  assert.equal(compressCalls, 2)
})

test('two compression failures retain the final error and input path', async () => {
  let compressCalls = 0

  const result = await runBatchCompressionItem(item, {
    isOutputValid: async () => false,
    compress: async () => {
      compressCalls += 1
      throw new Error(compressCalls === 1 ? 'first failure' : 'final ffmpeg failure')
    },
    isCancelled: () => false,
  })

  assert.deepEqual(result, {
    status: 'failed',
    failure: {
      inputPath: '/inputs/clip.mp4',
      message: 'final ffmpeg failure',
    },
  })
  assert.equal(compressCalls, 2)
})

test('cancellation error does not retry', async () => {
  let cancelled = false
  let compressCalls = 0

  const result = await runBatchCompressionItem(item, {
    isOutputValid: async () => false,
    compress: async () => {
      compressCalls += 1
      cancelled = true
      throw new Error('cancelled')
    },
    isCancelled: () => cancelled,
  })

  assert.deepEqual(result, { status: 'cancelled' })
  assert.equal(compressCalls, 1)
})

test('duplicate output paths abort before workers start', () => {
  let workersStarted = false

  assert.throws(
    () => {
      assertUniqueOutputPaths([
        '/outputs/clip_batch.mp4',
        '/outputs/clip_batch.mp4',
      ])
      workersStarted = true
    },
    /输出路径冲突.*clip_batch\.mp4/,
  )
  assert.equal(workersStarted, false)
})

test('macOS output paths differing only by case collide', () => {
  assert.throws(
    () =>
      assertUniqueOutputPaths(
        ['/outputs/Clip_batch.mp4', '/outputs/clip_batch.mp4'],
        'macos',
      ),
    /输出路径冲突/,
  )
})

test('Windows output paths differing only by case collide', () => {
  assert.throws(
    () =>
      assertUniqueOutputPaths(
        ['C:\\Outputs\\Clip_batch.mp4', 'c:\\outputs\\clip_batch.mp4'],
        'windows',
      ),
    /输出路径冲突/,
  )
})

test('Linux output paths differing only by case remain distinct', () => {
  assert.doesNotThrow(() =>
    assertUniqueOutputPaths(
      ['/outputs/Clip_batch.mp4', '/outputs/clip_batch.mp4'],
      'linux',
    ),
  )
})
