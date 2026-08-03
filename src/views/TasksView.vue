<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  cancelTask,
  clearFinishedTasks,
  removeTask,
  statusLabel,
  tasks,
  typeLabel,
  type AppTask,
  type TaskStatus,
} from '../taskStore'

type StatusFilter = 'all' | TaskStatus

const statusFilter = ref<StatusFilter>('all')

const filters: { id: StatusFilter; label: string }[] = [
  { id: 'all', label: '全部' },
  { id: 'running', label: '进行中' },
  { id: 'pending', label: '等待中' },
  { id: 'done', label: '已完成' },
  { id: 'error', label: '失败' },
  { id: 'cancelled', label: '已取消' },
]

const filteredTasks = computed(() => {
  if (statusFilter.value === 'all') return tasks.value
  return tasks.value.filter((t) => t.status === statusFilter.value)
})

const filterCounts = computed(() => {
  const counts: Record<StatusFilter, number> = {
    all: tasks.value.length,
    pending: 0,
    running: 0,
    done: 0,
    error: 0,
    cancelled: 0,
  }
  for (const t of tasks.value) {
    counts[t.status] += 1
  }
  return counts
})

const hasFinished = computed(() =>
  tasks.value.some((t) => t.status === 'done' || t.status === 'error' || t.status === 'cancelled'),
)

function isActive(status: TaskStatus) {
  return status === 'running' || status === 'pending'
}

function formatTime(iso: string) {
  try {
    return new Date(iso).toLocaleString()
  } catch {
    return iso
  }
}

async function onCancel(task: AppTask) {
  await cancelTask(task.id)
}

async function onDelete(task: AppTask) {
  await removeTask(task.id)
}

async function onClearFinished() {
  await clearFinishedTasks()
}
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1>任务列表</h1>
        <p>记录批量压制、视频合成与自动压制任务</p>
      </div>
      <button
        type="button"
        class="btn btn-ghost"
        :disabled="!hasFinished"
        @click="onClearFinished"
      >
        清空已结束
      </button>
    </header>

    <section class="panel">
      <div class="filter-bar">
        <button
          v-for="item in filters"
          :key="item.id"
          type="button"
          class="filter-chip"
          :class="{ active: statusFilter === item.id }"
          @click="statusFilter = item.id"
        >
          {{ item.label }}
          <span class="filter-count">{{ filterCounts[item.id] }}</span>
        </button>
      </div>

      <div class="list-section">
        <p v-if="!tasks.length" class="empty-hint">暂无任务记录</p>
        <p v-else-if="!filteredTasks.length" class="empty-hint">该状态下暂无任务</p>
        <ul v-else class="file-list task-list">
          <li v-for="task in filteredTasks" :key="task.id" class="file-item task-item">
            <div class="file-meta">
              <p class="file-name">{{ task.title }}</p>
              <p class="file-sub">
                <span class="type-tag">{{ typeLabel(task.type) }}</span>
                · {{ formatTime(task.updatedAt) }}
                <template v-if="task.error"> · {{ task.error }}</template>
              </p>
            </div>
            <div class="file-actions">
              <span class="status" :class="`status-${task.status}`">
                {{ statusLabel(task.status) }}
                <template v-if="task.status === 'running'"> {{ task.progress }}%</template>
              </span>
              <button
                v-if="isActive(task.status)"
                type="button"
                class="btn btn-ghost btn-sm"
                @click="onCancel(task)"
              >
                取消
              </button>
              <button type="button" class="btn btn-ghost btn-sm" @click="onDelete(task)">
                删除
              </button>
            </div>
            <div
              v-if="task.status === 'running' || task.status === 'done'"
              class="progress"
              aria-hidden="true"
            >
              <i :style="{ width: `${task.progress}%` }" />
            </div>
          </li>
        </ul>
      </div>
    </section>
  </div>
</template>

<style scoped>
.filter-bar {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  padding: 16px 18px 14px;
}

.filter-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 7px 12px;
  border-radius: 10px;
  border: 1px solid var(--line);
  background: rgba(8, 16, 15, 0.35);
  color: var(--text-muted);
  font-size: 0.85rem;
  font-weight: 600;
  transition:
    background 0.15s ease,
    border-color 0.15s ease,
    color 0.15s ease;
}

.filter-chip:hover {
  color: var(--text);
  border-color: var(--line-strong);
}

.filter-chip.active {
  color: var(--accent);
  border-color: rgba(94, 234, 212, 0.35);
  background: var(--accent-soft);
}

.filter-count {
  min-width: 1.25em;
  color: inherit;
  opacity: 0.72;
  font-variant-numeric: tabular-nums;
  font-weight: 500;
}

.task-list {
  max-height: none;
}
.task-item {
  grid-template-columns: 1fr auto;
}
.type-tag {
  color: var(--accent);
  font-weight: 600;
}
</style>
