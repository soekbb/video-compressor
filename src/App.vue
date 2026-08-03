<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { initAutoStore } from './autoStore'
import { currentPage, goToPage } from './navigation'
import { initSettings } from './settings'
import { initTaskStore } from './taskStore'
import type { AppPage } from './types'
import BatchCompressView from './views/BatchCompressView.vue'
import MergeView from './views/MergeView.vue'
import AutoCompressView from './views/AutoCompressView.vue'
import TasksView from './views/TasksView.vue'
import SettingsView from './views/SettingsView.vue'

const viewMap = {
  batch: BatchCompressView,
  merge: MergeView,
  auto: AutoCompressView,
  tasks: TasksView,
  settings: SettingsView,
} as const

const currentView = computed(() => viewMap[currentPage.value])

onMounted(() => {
  void initSettings()
  void initAutoStore()
  void initTaskStore()
})

const nav: { id: AppPage; label: string }[] = [
  { id: 'batch', label: '批量压制' },
  { id: 'merge', label: '视频合成' },
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
          @click="goToPage(item.id)"
        >
          {{ item.label }}
        </button>
      </nav>

      <p class="sidebar-foot">分辨率不变 · 画质优先</p>
    </aside>

    <main class="main">
      <KeepAlive>
        <component :is="currentView" :key="currentPage" />
      </KeepAlive>
    </main>
  </div>
</template>

<style scoped>
.shell {
  display: grid;
  grid-template-columns: 220px 1fr;
  width: min(1180px, calc(100% - 24px));
  min-height: calc(100vh - 24px);
  margin: 12px auto;
  border: 1px solid var(--line);
  border-radius: calc(var(--radius) + 6px);
  background: var(--bg-elevated);
  backdrop-filter: blur(16px);
  box-shadow: var(--shadow);
  overflow: hidden;
  animation: rise-in 0.45s ease both;
}

.sidebar {
  display: flex;
  flex-direction: column;
  gap: 20px;
  padding: 22px 14px;
  background: linear-gradient(180deg, rgba(15, 61, 62, 0.55), rgba(8, 16, 15, 0.35));
  border-right: 1px solid var(--line);
}

.brand-block {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 4px 8px 8px;
}

.brand-icon {
  width: 40px;
  height: 40px;
  border-radius: 12px;
  background: linear-gradient(145deg, #1a4d4a, #0f3d3e);
  box-shadow: inset 0 0 0 1px rgba(94, 234, 212, 0.25);
  display: grid;
  place-items: center;
  flex-shrink: 0;
}

.brand-icon svg {
  width: 22px;
  height: 22px;
}

.brand-name {
  font-family: Sora, "Noto Sans SC", sans-serif;
  font-size: 1.2rem;
  font-weight: 700;
  line-height: 1.15;
  letter-spacing: 0.02em;
}

.brand-by {
  margin-top: 4px;
  color: var(--text-dim);
  font-size: 0.75rem;
}

.nav {
  display: grid;
  gap: 4px;
  flex: 1;
  align-content: start;
}

.nav-item {
  text-align: left;
  padding: 9px 12px;
  border-radius: 10px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-muted);
  font-weight: 600;
  font-size: 0.92rem;
  line-height: 1.2;
  transition:
    background 0.15s ease,
    color 0.15s ease,
    border-color 0.15s ease;
}

.nav-item:hover {
  background: rgba(255, 255, 255, 0.04);
  color: var(--text);
}

.nav-item.active {
  background: var(--accent-soft);
  border-color: rgba(94, 234, 212, 0.28);
  color: var(--accent);
  box-shadow: inset 3px 0 0 var(--accent);
}

.sidebar-foot {
  margin: 0;
  padding: 0 8px;
  color: var(--text-dim);
  font-size: 0.75rem;
}

.main {
  min-width: 0;
  padding: 22px 22px 28px;
  overflow: auto;
}

@media (max-width: 860px) {
  .shell {
    grid-template-columns: 1fr;
    width: min(100% - 16px, 1180px);
  }

  .sidebar {
    border-right: none;
    border-bottom: 1px solid var(--line);
  }

  .nav {
    grid-template-columns: repeat(2, 1fr);
  }
}
</style>
