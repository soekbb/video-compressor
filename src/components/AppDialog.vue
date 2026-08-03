<script setup lang="ts">
import { computed, onBeforeUnmount, watch } from 'vue'
import { closeDialog, dialogState } from '../dialog'

const open = computed(() => dialogState.value.open)
const mode = computed(() => dialogState.value.mode)
const title = computed(() => dialogState.value.title)
const message = computed(() => dialogState.value.message)
const confirmText = computed(() => dialogState.value.confirmText)
const cancelText = computed(() => dialogState.value.cancelText)
const tone = computed(() => dialogState.value.tone)
const items = computed(() => dialogState.value.items)
const itemsMore = computed(() => dialogState.value.itemsMore)

function onKeydown(e: KeyboardEvent) {
  if (!open.value) return
  if (e.key === 'Escape') {
    e.preventDefault()
    closeDialog(mode.value !== 'confirm')
    return
  }
  if (e.key === 'Enter') {
    e.preventDefault()
    closeDialog(true)
  }
}

watch(open, (v) => {
  if (v) window.addEventListener('keydown', onKeydown)
  else window.removeEventListener('keydown', onKeydown)
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <Teleport to="body">
    <Transition name="dialog">
      <div
        v-if="open"
        class="dialog-root"
        role="presentation"
        @click.self="closeDialog(mode !== 'confirm')"
      >
        <div
          class="dialog-card"
          role="alertdialog"
          aria-modal="true"
          aria-labelledby="app-dialog-title"
          aria-describedby="app-dialog-message"
        >
          <div class="dialog-accent" :class="`tone-${tone}`" aria-hidden="true">
            <svg v-if="tone === 'ok'" viewBox="0 0 20 20" fill="none">
              <circle cx="10" cy="10" r="7.2" stroke="currentColor" stroke-width="1.4" />
              <path
                d="M6.6 10.2l2.2 2.2 4.6-4.8"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            <svg v-else-if="tone === 'warn'" viewBox="0 0 20 20" fill="none">
              <path
                d="M10 3.6 17.2 16H2.8L10 3.6Z"
                stroke="currentColor"
                stroke-width="1.4"
                stroke-linejoin="round"
              />
              <path d="M10 8v3.6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
              <circle cx="10" cy="14.2" r="0.9" fill="currentColor" />
            </svg>
            <svg v-else-if="tone === 'danger'" viewBox="0 0 20 20" fill="none">
              <circle cx="10" cy="10" r="7.2" stroke="currentColor" stroke-width="1.4" />
              <path
                d="M7.4 7.4l5.2 5.2M12.6 7.4l-5.2 5.2"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
              />
            </svg>
            <svg v-else viewBox="0 0 20 20" fill="none">
              <circle cx="10" cy="10" r="7.2" stroke="currentColor" stroke-width="1.4" />
              <path d="M10 6.4v5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
              <circle cx="10" cy="13.8" r="0.9" fill="currentColor" />
            </svg>
          </div>

          <div class="dialog-body">
            <h2 id="app-dialog-title" class="dialog-title">{{ title }}</h2>
            <p id="app-dialog-message" class="dialog-message">{{ message }}</p>

            <ul v-if="items.length" class="dialog-items">
              <li v-for="(item, index) in items" :key="`${item.title}-${index}`" class="dialog-item">
                <span class="item-index">{{ String(index + 1).padStart(2, '0') }}</span>
                <span class="item-folder" aria-hidden="true">
                  <svg viewBox="0 0 16 16" fill="none">
                    <path
                      d="M2.2 4.2A1.4 1.4 0 0 1 3.6 2.8h2.3l1.1 1.3h5.4A1.4 1.4 0 0 1 13.8 5.5v6.3a1.4 1.4 0 0 1-1.4 1.4H3.6a1.4 1.4 0 0 1-1.4-1.4V4.2Z"
                      stroke="currentColor"
                      stroke-width="1.2"
                      stroke-linejoin="round"
                    />
                  </svg>
                </span>
                <div class="item-meta">
                  <p class="item-title" :title="item.title">{{ item.title }}</p>
                  <p v-if="item.meta" class="item-sub">{{ item.meta }}</p>
                </div>
              </li>
              <li v-if="itemsMore > 0" class="dialog-item more">
                <span class="more-text">还有 {{ itemsMore }} 个剧目未展示</span>
              </li>
            </ul>
          </div>

          <div class="dialog-actions">
            <button
              v-if="mode === 'confirm'"
              type="button"
              class="btn btn-ghost dialog-cancel"
              @click="closeDialog(false)"
            >
              {{ cancelText }}
            </button>
            <button type="button" class="btn btn-primary dialog-confirm" @click="closeDialog(true)">
              {{ confirmText }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.dialog-root {
  position: fixed;
  inset: 0;
  z-index: 1000;
  display: grid;
  place-items: center;
  padding: 24px;
  background: rgba(4, 10, 10, 0.58);
  backdrop-filter: blur(8px);
}

.dialog-card {
  width: min(400px, 100%);
  padding: 22px 22px 18px;
  border-radius: 18px;
  border: 1px solid var(--line-strong);
  background:
    radial-gradient(420px 160px at 12% -20%, rgba(94, 234, 212, 0.12), transparent 55%),
    linear-gradient(180deg, rgba(26, 46, 44, 0.96), rgba(14, 22, 22, 0.96));
  box-shadow: 0 28px 70px rgba(0, 0, 0, 0.45);
}

.dialog-accent {
  width: 40px;
  height: 40px;
  margin-bottom: 14px;
  border-radius: 12px;
  display: grid;
  place-items: center;
  background: var(--accent-soft);
  color: var(--accent);
  box-shadow: inset 0 0 0 1px rgba(94, 234, 212, 0.22);
}

.dialog-accent svg {
  width: 20px;
  height: 20px;
}

.dialog-accent.tone-ok {
  color: var(--ok);
  background: rgba(110, 231, 183, 0.12);
  box-shadow: inset 0 0 0 1px rgba(110, 231, 183, 0.28);
}

.dialog-accent.tone-warn {
  color: var(--warn);
  background: rgba(240, 180, 41, 0.12);
  box-shadow: inset 0 0 0 1px rgba(240, 180, 41, 0.28);
}

.dialog-accent.tone-danger {
  color: var(--danger);
  background: rgba(248, 113, 113, 0.12);
  box-shadow: inset 0 0 0 1px rgba(248, 113, 113, 0.28);
}

.dialog-title {
  margin: 0 0 8px;
  font-family: Sora, "Noto Sans SC", sans-serif;
  font-size: 1.05rem;
  font-weight: 700;
  letter-spacing: 0.01em;
}

.dialog-message {
  margin: 0;
  color: var(--text-muted);
  font-size: 0.9rem;
  line-height: 1.55;
  white-space: pre-wrap;
  word-break: break-word;
}

.dialog-items {
  list-style: none;
  margin: 14px 0 0;
  padding: 0;
  display: grid;
  gap: 6px;
  max-height: 220px;
  overflow: auto;
}

.dialog-item {
  display: grid;
  grid-template-columns: auto auto 1fr;
  align-items: center;
  gap: 10px;
  min-height: 44px;
  padding: 8px 10px;
  border-radius: 12px;
  border: 1px solid rgba(148, 197, 189, 0.16);
  background: rgba(8, 16, 15, 0.55);
}

.dialog-item.more {
  grid-template-columns: 1fr;
  min-height: 34px;
  justify-items: center;
  border-style: dashed;
  background: transparent;
}

.item-index {
  font-family: Sora, sans-serif;
  font-size: 0.78rem;
  font-weight: 700;
  color: var(--accent);
  opacity: 0.9;
  font-variant-numeric: tabular-nums;
}

.item-folder {
  width: 28px;
  height: 28px;
  border-radius: 8px;
  display: grid;
  place-items: center;
  color: var(--accent);
  background: rgba(94, 234, 212, 0.1);
  box-shadow: inset 0 0 0 1px rgba(94, 234, 212, 0.18);
}

.item-folder svg {
  width: 14px;
  height: 14px;
}

.item-meta {
  min-width: 0;
}

.item-title {
  margin: 0;
  color: var(--text);
  font-size: 0.88rem;
  font-weight: 600;
  line-height: 1.25;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.item-sub {
  margin: 3px 0 0;
  color: var(--text-dim);
  font-size: 0.75rem;
  line-height: 1.2;
}

.more-text {
  color: var(--text-dim);
  font-size: 0.78rem;
}

.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 18px;
}

.dialog-cancel,
.dialog-confirm {
  min-width: 88px;
}

.dialog-enter-active,
.dialog-leave-active {
  transition: opacity 0.18s ease;
}

.dialog-enter-active .dialog-card,
.dialog-leave-active .dialog-card {
  transition:
    transform 0.2s ease,
    opacity 0.2s ease;
}

.dialog-enter-from,
.dialog-leave-to {
  opacity: 0;
}

.dialog-enter-from .dialog-card,
.dialog-leave-to .dialog-card {
  opacity: 0;
  transform: translateY(8px) scale(0.98);
}
</style>
