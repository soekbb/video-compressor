/// <reference types="node" />

import assert from 'node:assert/strict'
import test from 'node:test'
import { getCompressResolution, normalizeResolutionPreset } from './settings.ts'

test('compress default is 1080, illegal values use the given fallback', () => {
  assert.equal(getCompressResolution(), '1080')
  assert.equal(normalizeResolutionPreset('720', '1080'), '720')
  assert.equal(normalizeResolutionPreset('1080', '1080'), '1080')
  assert.equal(normalizeResolutionPreset('4k', '1080'), '1080')
  assert.equal(normalizeResolutionPreset('720p', '1080'), '1080')
  assert.equal(normalizeResolutionPreset('', '1080'), '1080')
})
