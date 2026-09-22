/// <reference types="node" />

import assert from 'node:assert/strict'
import test from 'node:test'
import { normalizeResolutionPreset } from './settings.ts'

test('compress fallback is 720, merge fallback is 1080', () => {
  assert.equal(normalizeResolutionPreset('720', '720'), '720')
  assert.equal(normalizeResolutionPreset('1080', '720'), '1080')
  assert.equal(normalizeResolutionPreset('4k', '720'), '720')
  assert.equal(normalizeResolutionPreset('720p', '1080'), '1080')
  assert.equal(normalizeResolutionPreset('', '1080'), '1080')
})
