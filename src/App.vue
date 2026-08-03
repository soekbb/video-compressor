<script setup lang="ts">
import { computed, onMounted, watch } from 'vue'
import { initAutoStore } from './autoStore'
import { currentPage, goToPage } from './navigation'
import { initSettings, qualityPresetLabel, settings } from './settings'
import { initNotifications, notifyDesktop } from './notify'
import {
  initTaskStore,
  statusLabel,
  taskReason,
  tasks,
  type AppTask,
  type TaskStatus,
} from './taskStore'
import { showToast } from './toast'
import type { AppPage } from './types'
import AppDialog from './components/AppDialog.vue'
import AppToast from './components/AppToast.vue'
import WorkbenchView from './views/WorkbenchView.vue'
import AutoCompressView from './views/AutoCompressView.vue'
import TasksView from './views/TasksView.vue'
import SettingsView from './views/SettingsView.vue'

const viewMap = {
  workbench: WorkbenchView,
  tasks: TasksView,
  settings: SettingsView,
} as const

const currentView = computed(() =>
  currentPage.value === 'auto'
    ? null
    : viewMap[currentPage.value as Exclude<AppPage, 'auto'>],
)

const activeTaskCount = computed(
  () => tasks.value.filter((t) => t.status === 'running' || t.status === 'pending').length,
)

const knownTaskStatus = new Map<string, TaskStatus>()
let taskWatchReady = false

function isEnded(status: TaskStatus) {
  return status === 'done' || status === 'error' || status === 'cancelled'
}

function toastTone(status: TaskStatus) {
  if (status === 'done') return 'ok' as const
  if (status === 'error') return 'danger' as const
  return 'warn' as const
}

function notifyTaskEnded(task: AppTask) {
  const label = statusLabel(task.status)
  const reason = taskReason(task)
  const title = '刚刚好影工'
  let body = `「${task.title}」${label}`
  if (task.status === 'error' && reason) body = `「${task.title}」失败：${reason}`
  else if (task.status === 'cancelled' && reason) body = `「${task.title}」${reason}`

  // 完成/失败：系统通知；取消仅应用内 toast（含原因）
  if (task.status === 'done' || task.status === 'error') {
    void notifyDesktop(title, body)
  }

  if (currentPage.value === 'tasks') return
  showToast({
    message: body,
    tone: toastTone(task.status),
    actionLabel: '查看',
    onAction: () => goToPage('tasks'),
  })
}

watch(
  tasks,
  (list) => {
    if (!taskWatchReady) {
      for (const t of list) knownTaskStatus.set(t.id, t.status)
      taskWatchReady = true
      return
    }
    const alive = new Set(list.map((t) => t.id))
    for (const id of [...knownTaskStatus.keys()]) {
      if (!alive.has(id)) knownTaskStatus.delete(id)
    }
    for (const task of list) {
      const prev = knownTaskStatus.get(task.id)
      if (prev && prev !== task.status && isEnded(task.status) && !isEnded(prev)) {
        notifyTaskEnded(task)
      }
      knownTaskStatus.set(task.id, task.status)
    }
  },
  { deep: true },
)

onMounted(async () => {
  await Promise.all([initSettings(), initAutoStore(), initTaskStore(), initNotifications()])
  for (const t of tasks.value) knownTaskStatus.set(t.id, t.status)
  taskWatchReady = true
})

const nav: { id: AppPage; label: string }[] = [
  { id: 'workbench', label: '压制合成' },
  { id: 'auto', label: '自动压制' },
  { id: 'tasks', label: '任务列表' },
  { id: 'settings', label: '系统设置' },
]
</script>

<template>
  <div class="shell">
    <aside class="sidebar">
      <div class="brand-block">
        <div class="brand-icon" aria-hidden="true">
          <svg viewBox="0 0 20 20" fill="none">
            <rect x="3.2" y="7.2" width="8.2" height="5.6" rx="1.1" fill="#245955" />
            <rect x="5.4" y="6" width="10.4" height="7.2" rx="1.3" fill="#E8F5F0" />
            <rect x="6.1" y="6.8" width="0.9" height="0.9" rx="0.2" fill="#0F3D3E" />
            <rect x="6.1" y="8.3" width="0.9" height="0.9" rx="0.2" fill="#0F3D3E" />
            <rect x="6.1" y="9.8" width="0.9" height="0.9" rx="0.2" fill="#0F3D3E" />
            <rect x="6.1" y="11.3" width="0.9" height="0.9" rx="0.2" fill="#0F3D3E" />
            <rect x="14.2" y="6.8" width="0.9" height="0.9" rx="0.2" fill="#0F3D3E" />
            <rect x="14.2" y="8.3" width="0.9" height="0.9" rx="0.2" fill="#0F3D3E" />
            <rect x="14.2" y="9.8" width="0.9" height="0.9" rx="0.2" fill="#0F3D3E" />
            <rect x="14.2" y="11.3" width="0.9" height="0.9" rx="0.2" fill="#0F3D3E" />
            <path d="M8.6 7.6V12.4L12.4 10L8.6 7.6Z" fill="#0F3D3E" />
            <circle cx="10" cy="15.4" r="0.85" fill="#5EEAD4" />
          </svg>
        </div>
        <div>
          <div class="brand-name">刚刚好影工</div>
          <div class="brand-by">视频压制 · 合成</div>
        </div>
      </div>

      <nav class="nav">
        <button
          v-for="item in nav"
          :key="item.id"
          type="button"
          class="nav-item"
          :class="{ active: currentPage === item.id }"
          :aria-current="currentPage === item.id ? 'page' : undefined"
          @click="goToPage(item.id)"
        >
          <span class="nav-icon" aria-hidden="true">
            <!-- 压制合成 -->
            <svg v-if="item.id === 'workbench'" viewBox="0 0 20 20" fill="none">
              <rect x="3" y="4.5" width="14" height="11" rx="2" stroke="currentColor" stroke-width="1.4" />
              <path d="M8 8.2v3.6l3.2-1.8L8 8.2Z" fill="currentColor" />
            </svg>
            <!-- 自动压制 -->
            <svg v-else-if="item.id === 'auto'" viewBox="0 0 20 20" fill="none">
              <path
                d="M10 3.2a6.8 6.8 0 1 0 6.5 4.7"
                stroke="currentColor"
                stroke-width="1.4"
                stroke-linecap="round"
              />
              <path
                d="M16.2 3.5v3.4h-3.4"
                stroke="currentColor"
                stroke-width="1.4"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            <!-- 任务列表 -->
            <svg v-else-if="item.id === 'tasks'" viewBox="0 0 20 20" fill="none">
              <path
                d="M4.5 5.5h11M4.5 10h11M4.5 14.5h7.5"
                stroke="currentColor"
                stroke-width="1.4"
                stroke-linecap="round"
              />
            </svg>
            <!-- 系统设置 -->
            <svg v-else viewBox="0 0 20 20" fill="none">
              <circle cx="10" cy="10" r="2.4" stroke="currentColor" stroke-width="1.4" />
              <path
                d="M10 3.2v1.6M10 15.2v1.6M3.2 10h1.6M15.2 10h1.6M5.2 5.2l1.1 1.1M13.7 13.7l1.1 1.1M14.8 5.2l-1.1 1.1M6.3 13.7l-1.1 1.1"
                stroke="currentColor"
                stroke-width="1.4"
                stroke-linecap="round"
              />
            </svg>
          </span>
          <span class="nav-label">{{ item.label }}</span>
          <span
            v-if="item.id === 'tasks' && activeTaskCount > 0"
            class="nav-badge"
            :aria-label="`${activeTaskCount} 个进行中`"
          >
            {{ activeTaskCount > 99 ? '99+' : activeTaskCount }}
          </span>
        </button>
      </nav>

      <p class="sidebar-foot">分辨率不变 · {{ qualityPresetLabel(settings.qualityPreset) }}</p>
    </aside>

    <main class="main">
      <!-- 始终挂载，以便启动时即可按设置开启自动扫描 -->
      <AutoCompressView v-show="currentPage === 'auto'" />
      <KeepAlive>
        <component
          v-if="currentView"
          :is="currentView"
          :key="currentPage"
        />
      </KeepAlive>
    </main>
    <AppDialog />
    <AppToast />
  </div>
</template>

<style scoped>
.shell {
  display: grid;
  grid-template-columns: 188px 1fr;
  width: 100%;
  min-height: 100vh;
  border: none;
  border-radius: 0;
  background: rgba(18, 28, 27, 0.55);
  overflow: hidden;
}

.sidebar {
  display: flex;
  flex-direction: column;
  gap: 18px;
  padding: 16px 10px 14px;
  border-right: 1px solid var(--line);
  background: linear-gradient(180deg, rgba(18, 32, 31, 0.98), rgba(12, 20, 20, 0.94));
}

.brand-block {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 2px 6px 4px;
}

.brand-icon {
  width: 34px;
  height: 34px;
  border-radius: 10px;
  display: grid;
  place-items: center;
  flex-shrink: 0;
  background: linear-gradient(145deg, #1f3d3a, #122422);
  border: 1px solid rgba(94, 234, 212, 0.22);
}

.brand-icon svg {
  width: 18px;
  height: 18px;
}

.brand-name {
  font-family: Sora, "Noto Sans SC", sans-serif;
  font-size: 1.02rem;
  font-weight: 700;
  letter-spacing: 0.01em;
  line-height: 1.15;
}

.brand-by {
  margin-top: 2px;
  color: var(--text-dim);
  font-size: 0.7rem;
}

.nav {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex: 1;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 36px;
  padding: 0 10px;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: var(--text-muted);
  text-align: left;
  font-size: 0.86rem;
  font-weight: 550;
  line-height: 1;
  transition:
    background 0.14s ease,
    color 0.14s ease;
}

.nav-icon {
  width: 18px;
  height: 18px;
  display: grid;
  place-items: center;
  flex-shrink: 0;
  opacity: 0.88;
}

.nav-icon svg {
  width: 18px;
  height: 18px;
  display: block;
}

.nav-label {
  min-width: 0;
  flex: 1;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.nav-badge {
  min-width: 18px;
  height: 18px;
  padding: 0 5px;
  border-radius: 999px;
  display: inline-grid;
  place-items: center;
  background: rgba(94, 234, 212, 0.18);
  color: var(--accent);
  font-size: 0.68rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  line-height: 1;
}

.nav-item:hover {
  color: var(--text);
  background: rgba(255, 255, 255, 0.04);
}

.nav-item.active {
  color: var(--text);
  background: rgba(94, 234, 212, 0.12);
}

.nav-item.active .nav-icon {
  color: var(--accent);
  opacity: 1;
}

.nav-item.active .nav-badge {
  background: rgba(94, 234, 212, 0.28);
}

.sidebar-foot {
  margin: 0;
  padding: 8px 8px 2px;
  color: var(--text-dim);
  font-size: 0.7rem;
  line-height: 1.35;
}

.main {
  min-width: 0;
  padding: 22px 28px 28px;
  overflow: auto;
}

@media (max-width: 860px) {
  .shell {
    grid-template-columns: 1fr;
    min-height: 100vh;
  }

  .sidebar {
    border-right: none;
    border-bottom: 1px solid var(--line);
    gap: 12px;
    padding-bottom: 12px;
  }

  .nav {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 4px;
    flex: none;
  }
}
</style>
