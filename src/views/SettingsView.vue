<script setup lang="ts">
import { ref, watch } from 'vue'
import { settings, settingsReady, updateSettings } from '../settings'

const savedTip = ref(false)
const concurrencyInput = ref(String(settings.value.concurrency))
const intervalInput = ref(String(settings.value.scanIntervalMinutes))
let tipTimer: number | undefined

watch(
  [settings, settingsReady],
  () => {
    concurrencyInput.value = String(settings.value.concurrency)
    intervalInput.value = String(settings.value.scanIntervalMinutes)
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

async function onSave() {
  try {
    await updateSettings({
      concurrency: Number(concurrencyInput.value),
      scanIntervalMinutes: Number(intervalInput.value),
    })
    concurrencyInput.value = String(settings.value.concurrency)
    intervalInput.value = String(settings.value.scanIntervalMinutes)
    saveFlash()
  } catch {
    // persist error already logged
  }
}
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1>系统设置</h1>
        <p>调整并行压制数量与自动扫描间隔</p>
      </div>
    </header>

    <section class="panel settings-panel">
      <div class="setting-row">
        <div class="setting-meta">
          <h3>并行压制数量</h3>
          <p>同时压制的视频数，最大 5；自动压制时也按此并行处理同一剧目内视频</p>
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

      <div class="setting-row">
        <div class="setting-meta">
          <h3>画质档位</h3>
          <p>当前固定为画质优先（x264 CRF 18，分辨率不变）</p>
        </div>
        <div class="setting-control">
          <div class="preset-pill control-pill">画质优先</div>
        </div>
      </div>

      <div class="settings-foot">
        <p v-if="savedTip" class="saved">已保存</p>
        <button type="button" class="btn btn-primary" @click="onSave">保存设置</button>
      </div>
    </section>
  </div>
</template>

<style scoped>
.settings-panel {
  padding: 4px 0 8px;
  overflow: hidden;
}

.setting-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 148px;
  align-items: center;
  column-gap: 32px;
  padding: 22px 26px;
  border-bottom: 1px solid var(--line);
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

.setting-control {
  width: 148px;
  display: flex;
  justify-content: flex-end;
  align-items: center;
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
  /* hide native steppers for cleaner alignment */
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

.control-pill {
  width: 148px;
  height: 42px;
  justify-content: center;
  border-radius: 11px;
  box-sizing: border-box;
}

.settings-foot {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  gap: 14px;
  padding: 20px 26px 16px;
}

.saved {
  margin: 0;
  color: var(--ok);
  font-size: 0.9rem;
}

@media (max-width: 720px) {
  .setting-row {
    grid-template-columns: 1fr;
    row-gap: 14px;
    align-items: start;
  }

  .setting-control,
  .num-field,
  .control-pill {
    width: 100%;
  }
}
</style>
