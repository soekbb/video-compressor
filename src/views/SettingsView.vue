<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { settings, settingsReady, updateSettings } from '../settings'
import {
  formatMemory,
  loadSystemInfo,
  recommendConcurrency,
} from '../systemInfo'
import { showToast } from '../toast'
import type { QualityPreset, SystemInfo } from '../types'

const savedTip = ref(false)
const concurrencyInput = ref(String(settings.value.concurrency))
const intervalInput = ref(String(settings.value.scanIntervalMinutes))
const qualityPreset = ref<QualityPreset>(settings.value.qualityPreset)
const autoScanOnLaunch = ref(settings.value.autoScanOnLaunch)
const systemInfo = ref<SystemInfo | null>(null)
let tipTimer: number | undefined
let saveTimer: number | undefined
let syncing = false

const recommendedConcurrency = computed(() =>
  systemInfo.value ? recommendConcurrency(systemInfo.value.cpuCores) : null,
)

const concurrencyHint = computed(() => {
  if (!systemInfo.value || recommendedConcurrency.value == null) return null
  const cores = systemInfo.value.cpuCores
  const n = recommendedConcurrency.value
  return `本机 ${cores} 核，建议并行 ${n} 路（压制较吃 CPU，留余量给系统）`
})

const memoryLabel = computed(() => formatMemory(systemInfo.value?.totalMemoryBytes))

onMounted(() => {
  void loadSystemInfo().then((info) => {
    systemInfo.value = info
  })
})

watch(
  [settings, settingsReady],
  () => {
    syncing = true
    concurrencyInput.value = String(settings.value.concurrency)
    intervalInput.value = String(settings.value.scanIntervalMinutes)
    qualityPreset.value = settings.value.qualityPreset
    autoScanOnLaunch.value = settings.value.autoScanOnLaunch
    queueMicrotask(() => {
      syncing = false
    })
  },
  { deep: true },
)

function saveFlash() {
  savedTip.value = true
  if (tipTimer) window.clearTimeout(tipTimer)
  tipTimer = window.setTimeout(() => {
    savedTip.value = false
  }, 1600)
}

function draftEqualsSaved() {
  if (!concurrencyInput.value.trim() || !intervalInput.value.trim()) return true
  const concurrency = Math.min(5, Math.max(1, Math.round(Number(concurrencyInput.value)) || 0))
  const interval = Math.min(
    60,
    Math.max(3, Math.round(Number(intervalInput.value)) || 0),
  )
  if (!Number.isFinite(concurrency) || !Number.isFinite(interval)) return true
  return (
    concurrency === settings.value.concurrency &&
    interval === settings.value.scanIntervalMinutes &&
    qualityPreset.value === settings.value.qualityPreset &&
    autoScanOnLaunch.value === settings.value.autoScanOnLaunch
  )
}

async function persistNow() {
  if (draftEqualsSaved()) return
  syncing = true
  try {
    await updateSettings({
      concurrency: Number(concurrencyInput.value),
      scanIntervalMinutes: Number(intervalInput.value),
      qualityPreset: qualityPreset.value,
      autoScanOnLaunch: autoScanOnLaunch.value,
    })
    concurrencyInput.value = String(settings.value.concurrency)
    intervalInput.value = String(settings.value.scanIntervalMinutes)
    qualityPreset.value = settings.value.qualityPreset
    autoScanOnLaunch.value = settings.value.autoScanOnLaunch
    saveFlash()
  } catch {
    showToast({ message: '设置保存失败', tone: 'danger' })
  } finally {
    queueMicrotask(() => {
      syncing = false
    })
  }
}

function scheduleSave() {
  if (syncing || !settingsReady.value || draftEqualsSaved()) return
  if (saveTimer) window.clearTimeout(saveTimer)
  saveTimer = window.setTimeout(() => {
    saveTimer = undefined
    void persistNow()
  }, 280)
}

watch([concurrencyInput, intervalInput, qualityPreset, autoScanOnLaunch], () => {
  scheduleSave()
})

async function setQuality(preset: QualityPreset) {
  if (qualityPreset.value === preset) return
  qualityPreset.value = preset
}

async function setAutoScan(enabled: boolean) {
  if (autoScanOnLaunch.value === enabled) return
  autoScanOnLaunch.value = enabled
}
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1>系统设置</h1>
        <p>本机配置一览，调整并行压制与其它偏好 · 修改后自动保存</p>
      </div>
      <p v-if="savedTip" class="saved-pill" aria-live="polite">已保存</p>
    </header>

    <section class="panel settings-panel">
      <div class="machine-block">
        <div class="machine-head">
          <div>
            <h3>本机信息</h3>
            <p>根据当前电脑配置给出并行压制建议，仅供参考</p>
          </div>
          <p v-if="systemInfo" class="machine-os">
            {{ systemInfo.os }}
            <span aria-hidden="true">·</span>
            {{ systemInfo.arch }}
          </p>
        </div>

        <div v-if="systemInfo" class="machine-body">
          <div class="machine-metrics">
            <div class="metric">
              <span class="metric-value">{{ systemInfo.cpuCores }}</span>
              <span class="metric-unit">核</span>
              <span class="metric-label">逻辑核心</span>
            </div>
            <div class="metric metric-accent" v-if="recommendedConcurrency != null">
              <span class="metric-value">{{ recommendedConcurrency }}</span>
              <span class="metric-unit">路</span>
              <span class="metric-label">建议并行</span>
            </div>
          </div>

          <ul class="machine-specs">
            <li>
              <span class="spec-label">处理器</span>
              <span class="spec-value">{{ systemInfo.cpuBrand || systemInfo.arch }}</span>
            </li>
            <li v-if="memoryLabel">
              <span class="spec-label">内存</span>
              <span class="spec-value">{{ memoryLabel }}</span>
            </li>
          </ul>
        </div>
        <p v-else class="machine-loading">正在读取本机配置…</p>
      </div>

      <div class="setting-row">
        <div class="setting-meta">
          <h3>并行压制数量</h3>
          <p>
            同时压制的视频数，最大 5；自动压制时也按此并行处理同一剧目内视频
            <template v-if="concurrencyHint">
              <br />
              {{ concurrencyHint }}
            </template>
          </p>
        </div>
        <div class="setting-control">
          <label class="num-field">
            <input
              v-model="concurrencyInput"
              class="num-input"
              type="number"
              min="1"
              max="5"
              step="1"
            />
            <span class="num-unit">个</span>
          </label>
        </div>
      </div>

      <div class="setting-row">
        <div class="setting-meta">
          <h3>自动扫描间隔</h3>
          <p>自动压制监控目录的轮询周期，范围 3–60 分钟</p>
        </div>
        <div class="setting-control">
          <label class="num-field">
            <input
              v-model="intervalInput"
              class="num-input"
              type="number"
              min="3"
              max="60"
              step="1"
            />
            <span class="num-unit">分钟</span>
          </label>
        </div>
      </div>

      <div class="setting-row setting-row-wide">
        <div class="setting-meta">
          <h3>画质档位</h3>
          <p>
            体积优先：x264 CRF 23 + faster，体积适中、速度更快；画质优先：CRF 18，更清晰。分辨率均不变
          </p>
        </div>
        <div class="setting-control setting-control-wide">
          <div class="preset-switch" role="group" aria-label="画质档位">
            <button
              type="button"
              class="preset-option"
              :class="{ active: qualityPreset === 'size' }"
              @click="setQuality('size')"
            >
              体积优先
            </button>
            <button
              type="button"
              class="preset-option"
              :class="{ active: qualityPreset === 'quality' }"
              @click="setQuality('quality')"
            >
              画质优先
            </button>
          </div>
        </div>
      </div>

      <div class="setting-row setting-row-wide last">
        <div class="setting-meta">
          <h3>启动时自动扫描</h3>
          <p>
            打开软件后自动开启自动压制扫描（需已设置监控目录；目录在「自动压制」页选择并自动保存）
          </p>
        </div>
        <div class="setting-control setting-control-wide">
          <div class="preset-switch" role="group" aria-label="启动时自动扫描">
            <button
              type="button"
              class="preset-option"
              :class="{ active: !autoScanOnLaunch }"
              @click="setAutoScan(false)"
            >
              关闭
            </button>
            <button
              type="button"
              class="preset-option"
              :class="{ active: autoScanOnLaunch }"
              @click="setAutoScan(true)"
            >
              开启
            </button>
          </div>
        </div>
      </div>
    </section>
  </div>
</template>

<style scoped>
.settings-panel {
  padding: 4px 0 8px;
  overflow: hidden;
}

.saved-pill {
  margin: 0;
  padding: 6px 12px;
  border-radius: 999px;
  background: rgba(110, 231, 183, 0.12);
  color: var(--ok);
  font-size: 0.82rem;
  font-weight: 650;
  white-space: nowrap;
}

.setting-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 148px;
  align-items: center;
  column-gap: 32px;
  padding: 22px 26px;
  border-bottom: 1px solid var(--line);
}

.setting-row.last {
  border-bottom: none;
}

.setting-row-wide {
  grid-template-columns: minmax(0, 1fr) minmax(220px, 260px);
}

.setting-meta {
  min-width: 0;
}

.setting-meta h3 {
  margin: 0 0 6px;
  font-size: 0.98rem;
  font-weight: 650;
  letter-spacing: 0.01em;
}

.setting-meta p {
  margin: 0;
  color: var(--text-dim);
  font-size: 0.84rem;
  line-height: 1.45;
  max-width: 36em;
}

.machine-block {
  padding: 22px 26px 24px;
  border-bottom: 1px solid var(--line);
  background:
    linear-gradient(135deg, rgba(94, 234, 212, 0.07), transparent 42%),
    linear-gradient(180deg, rgba(8, 16, 15, 0.35), transparent 100%);
}

.machine-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 18px;
}

.machine-head h3 {
  margin: 0 0 6px;
  font-size: 0.98rem;
  font-weight: 650;
  letter-spacing: 0.01em;
}

.machine-head p {
  margin: 0;
  color: var(--text-dim);
  font-size: 0.84rem;
  line-height: 1.45;
}

.machine-os {
  flex-shrink: 0;
  margin: 2px 0 0;
  padding: 5px 10px;
  border-radius: 999px;
  border: 1px solid rgba(94, 234, 212, 0.22);
  background: rgba(8, 16, 15, 0.45);
  color: var(--accent);
  font-size: 0.78rem;
  font-weight: 650;
  letter-spacing: 0.02em;
  white-space: nowrap;
}

.machine-os span {
  margin: 0 0.35em;
  opacity: 0.55;
}

.machine-body {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  gap: 16px 28px;
  align-items: stretch;
}

.machine-metrics {
  display: flex;
  gap: 12px;
}

.metric {
  position: relative;
  min-width: 112px;
  padding: 14px 16px 12px;
  border-radius: 14px;
  border: 1px solid var(--line-strong);
  background: rgba(8, 16, 15, 0.55);
  display: grid;
  grid-template-columns: auto auto;
  grid-template-rows: auto auto;
  column-gap: 4px;
  align-items: end;
}

.metric-accent {
  border-color: rgba(94, 234, 212, 0.38);
  background:
    linear-gradient(160deg, rgba(94, 234, 212, 0.14), rgba(8, 16, 15, 0.55) 70%);
  box-shadow: inset 0 0 0 1px rgba(94, 234, 212, 0.08);
}

.metric-value {
  grid-column: 1;
  grid-row: 1;
  font-size: 1.85rem;
  font-weight: 750;
  line-height: 1;
  letter-spacing: -0.03em;
  font-variant-numeric: tabular-nums;
  color: var(--text);
}

.metric-accent .metric-value {
  color: var(--accent);
}

.metric-unit {
  grid-column: 2;
  grid-row: 1;
  padding-bottom: 3px;
  color: var(--text-muted);
  font-size: 0.82rem;
  font-weight: 650;
}

.metric-label {
  grid-column: 1 / -1;
  grid-row: 2;
  margin-top: 8px;
  color: var(--text-dim);
  font-size: 0.76rem;
  font-weight: 600;
  letter-spacing: 0.04em;
}

.machine-specs {
  list-style: none;
  margin: 0;
  padding: 4px 0;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 12px;
  min-width: 0;
}

.machine-specs li {
  display: grid;
  grid-template-columns: 4.5em minmax(0, 1fr);
  gap: 10px;
  align-items: baseline;
}

.spec-label {
  color: var(--text-muted);
  font-size: 0.78rem;
  font-weight: 600;
}

.spec-value {
  color: var(--text);
  font-size: 0.92rem;
  font-weight: 650;
  line-height: 1.35;
  word-break: break-word;
}

.machine-loading {
  margin: 0;
  color: var(--text-muted);
  font-size: 0.84rem;
}

.setting-control {
  width: 148px;
  display: flex;
  justify-content: flex-end;
  align-items: center;
}

.setting-control-wide {
  width: 100%;
}

.num-field {
  position: relative;
  display: block;
  width: 148px;
}

.num-input {
  width: 100%;
  height: 42px;
  padding: 0 52px 0 14px;
  border-radius: 11px;
  border: 1px solid var(--line-strong);
  background: rgba(8, 16, 15, 0.55);
  color: var(--text);
  font-size: 0.95rem;
  font-weight: 650;
  font-variant-numeric: tabular-nums;
  text-align: left;
  outline: none;
  transition:
    border-color 0.15s ease,
    box-shadow 0.15s ease,
    background 0.15s ease;
  -moz-appearance: textfield;
}

.num-input::-webkit-outer-spin-button,
.num-input::-webkit-inner-spin-button {
  -webkit-appearance: none;
  margin: 0;
}

.num-input:hover {
  border-color: rgba(94, 234, 212, 0.35);
}

.num-input:focus {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px rgba(94, 234, 212, 0.12);
  background: rgba(8, 16, 15, 0.72);
}

.num-unit {
  position: absolute;
  right: 14px;
  top: 50%;
  transform: translateY(-50%);
  color: var(--text-muted);
  font-size: 0.82rem;
  font-weight: 500;
  pointer-events: none;
  line-height: 1;
}

.preset-switch {
  display: grid;
  grid-template-columns: 1fr 1fr;
  width: 100%;
  padding: 3px;
  border-radius: 12px;
  border: 1px solid var(--line-strong);
  background: rgba(8, 16, 15, 0.55);
  gap: 3px;
}

.preset-option {
  height: 36px;
  border: none;
  border-radius: 9px;
  background: transparent;
  color: var(--text-muted);
  font-size: 0.86rem;
  font-weight: 650;
  transition:
    background 0.15s ease,
    color 0.15s ease;
}

.preset-option:hover {
  color: var(--text);
}

.preset-option.active {
  background: var(--accent-soft);
  color: var(--accent);
  box-shadow: inset 0 0 0 1px rgba(94, 234, 212, 0.28);
}

@media (max-width: 720px) {
  .setting-row,
  .setting-row-wide {
    grid-template-columns: 1fr;
    row-gap: 14px;
    align-items: start;
  }

  .setting-control,
  .num-field,
  .setting-control-wide {
    width: 100%;
  }

  .machine-head {
    flex-direction: column;
    gap: 10px;
  }

  .machine-body {
    grid-template-columns: 1fr;
    gap: 14px;
  }

  .machine-metrics {
    width: 100%;
  }

  .metric {
    flex: 1;
    min-width: 0;
  }
}
</style>
