<script setup lang="ts">
import { dismissToast, runToastAction, toastList } from '../toast'

function toneClass(tone: string) {
  return `tone-${tone}`
}
</script>

<template>
  <Teleport to="body">
    <div class="toast-stack" aria-live="polite">
      <TransitionGroup name="toast">
        <div
          v-for="item in toastList"
          :key="item.id"
          class="toast"
          :class="toneClass(item.tone)"
          role="status"
        >
          <p class="toast-message">{{ item.message }}</p>
          <div class="toast-actions">
            <button
              v-if="item.actionLabel"
              type="button"
              class="toast-action"
              @click="runToastAction(item.id)"
            >
              {{ item.actionLabel }}
            </button>
            <button
              type="button"
              class="toast-dismiss"
              aria-label="关闭"
              @click="dismissToast(item.id)"
            >
              ×
            </button>
          </div>
        </div>
      </TransitionGroup>
    </div>
  </Teleport>
</template>

<style scoped>
.toast-stack {
  position: fixed;
  right: 20px;
  bottom: 20px;
  z-index: 1100;
  display: grid;
  gap: 8px;
  width: min(360px, calc(100vw - 32px));
  pointer-events: none;
}

.toast {
  pointer-events: auto;
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
  padding: 12px 12px 12px 14px;
  border-radius: 12px;
  border: 1px solid var(--line-strong);
  background: linear-gradient(180deg, rgba(26, 46, 44, 0.96), rgba(14, 22, 22, 0.96));
  box-shadow: 0 16px 40px rgba(0, 0, 0, 0.35);
}

.toast.tone-ok {
  border-color: rgba(110, 231, 183, 0.35);
}

.toast.tone-warn {
  border-color: rgba(240, 180, 41, 0.35);
}

.toast.tone-danger {
  border-color: rgba(248, 113, 113, 0.35);
}

.toast-message {
  margin: 0;
  flex: 1;
  min-width: 0;
  font-size: 0.88rem;
  line-height: 1.45;
  color: var(--text);
}

.toast-actions {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}

.toast-action {
  height: 28px;
  padding: 0 10px;
  border: none;
  border-radius: 8px;
  background: rgba(94, 234, 212, 0.14);
  color: var(--accent);
  font-size: 0.78rem;
  font-weight: 650;
}

.toast-action:hover {
  background: rgba(94, 234, 212, 0.22);
}

.toast-dismiss {
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: var(--text-dim);
  font-size: 1.1rem;
  line-height: 1;
}

.toast-dismiss:hover {
  color: var(--text);
  background: rgba(255, 255, 255, 0.06);
}

.toast-enter-active,
.toast-leave-active {
  transition:
    opacity 0.2s ease,
    transform 0.2s ease;
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateY(8px);
}

.toast-move {
  transition: transform 0.2s ease;
}
</style>
