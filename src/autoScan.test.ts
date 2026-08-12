/// <reference types="node" />
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  TWO_DAYS_MS,
  isDramaFolderFresh,
  pendingVideosForDrama,
  seedVideoNames,
} from './autoScan.ts'

test('folder newer than 2 days is fresh', () => {
  const now = 1_700_000_000_000
  assert.equal(isDramaFolderFresh(now - TWO_DAYS_MS + 1, now), true)
  assert.equal(isDramaFolderFresh(now - TWO_DAYS_MS - 1, now), false)
})

test('new drama (null record) returns all videos', () => {
  const folder = { videos: [{ name: 'a.mp4' }, { name: 'b.mp4' }] }
  assert.deepEqual(
    pendingVideosForDrama(folder, null).map((v) => v.name),
    ['a.mp4', 'b.mp4'],
  )
})

test('old drama pending excludes successful names', () => {
  const folder = { videos: [{ name: 'a.mp4' }, { name: 'b.mp4' }, { name: 'c.mp4' }] }
  assert.deepEqual(
    pendingVideosForDrama(folder, { videoNames: ['a.mp4', 'c.mp4'] }).map((v) => v.name),
    ['b.mp4'],
  )
})

test('missing videoNames treated as empty for pending (caller seeds separately)', () => {
  const folder = { videos: [{ name: 'a.mp4' }] }
  assert.deepEqual(
    pendingVideosForDrama(folder, { videoNames: undefined }).map((v) => v.name),
    ['a.mp4'],
  )
})

test('seedVideoNames lists all current names', () => {
  assert.deepEqual(seedVideoNames({ videos: [{ name: 'x.mp4' }, { name: 'y.mp4' }] }), [
    'x.mp4',
    'y.mp4',
  ])
})
