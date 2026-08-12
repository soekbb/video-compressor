/// <reference types="node" />

import assert from 'node:assert/strict'
import test from 'node:test'
import { createAsyncMutex } from './asyncMutex.ts'
import { mergeAppendDramaVideoSuccess } from './autoPersistence.ts'
import type { AutoRecord } from './types.ts'

test('overlapping append RMW without mutex drops a sibling name', async () => {
  let records: AutoRecord[] = []
  let releaseFirstRead!: () => void
  let releaseSecondRead!: () => void
  const firstReadDone = new Promise<void>((r) => {
    releaseFirstRead = r
  })
  const bothHaveRead = new Promise<void>((r) => {
    releaseSecondRead = r
  })

  const p1 = (async () => {
    const snapshot = records
    releaseFirstRead()
    await bothHaveRead
    records = mergeAppendDramaVideoSuccess(snapshot, {
      path: '/d',
      name: 'D',
      videoName: 'a.mp4',
      at: 't1',
    })
  })()

  const p2 = (async () => {
    await firstReadDone
    const snapshot = records
    releaseSecondRead()
    records = mergeAppendDramaVideoSuccess(snapshot, {
      path: '/d',
      name: 'D',
      videoName: 'b.mp4',
      at: 't2',
    })
  })()

  await Promise.all([p1, p2])
  assert.equal(records[0]?.videoNames?.length, 1)
})

test('mutex + delayed persist keeps both concurrent append names', async () => {
  let records: AutoRecord[] = []
  const runExclusive = createAsyncMutex()

  async function lockedAppend(videoName: string) {
    await runExclusive(async () => {
      const snapshot = records
      await new Promise((r) => setTimeout(r, 15))
      records = mergeAppendDramaVideoSuccess(snapshot, {
        path: '/d',
        name: 'D',
        videoName,
        at: 't',
      })
    })
  }

  await Promise.all([lockedAppend('a.mp4'), lockedAppend('b.mp4')])
  assert.deepEqual(new Set(records[0]?.videoNames), new Set(['a.mp4', 'b.mp4']))
  assert.equal(records[0]?.videoCount, 2)
})
