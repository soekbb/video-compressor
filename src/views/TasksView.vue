<script setup lang="ts">
import { computed, ref } from 'vue'
import { revealPath } from '../desktop'
import { showConfirm } from '../dialog'
import { showToast } from '../toast'
import {
  cancelTask,
  clearFinishedTasks,
  formatDuration,
  parseTaskMeta,
  removeTask,
  resolveDurationMs,
  statusLabel,
  taskReason,
  tasks,
  typeLabel,
  type AppTask,
  type TaskStatus,
} from '../taskStore'

type StatusFilter = 'all' | 'active' | 'ended'

const statusFilter = ref<StatusFilter>('all')

const filters: { id: StatusFilter; label: string }[] = [
  { id: 'all', label: '全部' },
  { id: 'active', label: '进行中' },
  { id: 'ended', label: '已结束' },
]

function isActive(status: TaskStatus) {
  return status === 'running' || status === 'pending'
}

function isEnded(status: TaskStatus) {
  return status === 'done' || status === 'error' || status === 'cancelled'
}

const filteredTasks = computed(() => {
  if (statusFilter.value === 'all') return tasks.value
  if (statusFilter.value === 'active') return tasks.value.filter((t) => isActive(t.status))
  return tasks.value.filter((t) => isEnded(t.status))
})

const filterCounts = computed(() => {
  let active = 0
  let ended = 0
  for (const t of tasks.value) {
    if (isActive(t.status)) active += 1
    else if (isEnded(t.status)) ended += 1
  }
  return {
    all: tasks.value.length,
    active,
    ended,
  }
})

const hasFinished = computed(() => tasks.value.some((t) => isEnded(t.status)))

function formatTime(iso: string) {
  try {
    return new Date(iso).toLocaleString()
  } catch {
    return iso
  }
}

function unitLabel(type: AppTask['type']) {
  return type === 'merge' ? '个片段' : '个视频'
}

function countText(task: AppTask) {
  const meta = parseTaskMeta(task.meta)
  const total = Number(meta.videoCount) || 0
  const done = Number(meta.doneCount) || 0
  if (!total) return ''
  if (task.type === 'merge' && (task.status === 'running' || task.status === 'pending')) {
    return `${total} ${unitLabel(task.type)}`
  }
  if (task.status === 'done') {
    return `${total} ${unitLabel(task.type)}`
  }
  return `${done} / ${total} ${unitLabel(task.type)}`
}

function durationText(task: AppTask) {
  return formatDuration(resolveDurationMs(task))
}

function footerText(task: AppTask) {
  if (isEnded(task.status)) {
    const finished = parseTaskMeta(task.meta).finishedAt || task.updatedAt
    return `完成于 ${formatTime(String(finished))}`
  }
  return `开始于 ${formatTime(task.createdAt)}`
}

function outputTarget(task: AppTask) {
  const meta = parseTaskMeta(task.meta)
  return String(meta.outputPath || meta.outputDir || '')
}

async function onCancel(task: AppTask) {
  const ok = await showConfirm({
    title: '取消任务',
    tone: 'warn',
    message: `确定取消「${task.title}」？进行中的压制将立即停止。`,
    confirmText: '确认取消',
    cancelText: '继续任务',
  })
  if (!ok) return
  await cancelTask(task.id)
}

async function onDelete(task: AppTask) {
  const active = isActive(task.status)
  const ok = await showConfirm({
    title: active ? '删除进行中的任务？' : '删除任务',
    tone: active ? 'danger' : 'warn',
    message: active
      ? `「${task.title}」正在进行，删除将同时取消并移除记录。`
      : `确定删除「${task.title}」？`,
    confirmText: '删除',
    cancelText: '取消',
  })
  if (!ok) return
  await removeTask(task.id)
}

async function onClearFinished() {
  const ok = await showConfirm({
    title: '清空已结束任务',
    tone: 'warn',
    message: `将删除 ${filterCounts.value.ended} 条已结束记录，不可恢复。`,
    confirmText: '清空',
    cancelText: '取消',
  })
  if (!ok) return
  await clearFinishedTasks()
}

async function onReveal(task: AppTask) {
  const target = outputTarget(task)
  if (!target) return
  try {
    await revealPath(target)
  } catch (e) {
    showToast({
      message: e instanceof Error ? e.message : String(e),
      tone: 'danger',
    })
  }
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
        class="btn btn-soft"
        :disabled="!hasFinished"
        @click="onClearFinished"
      >
        清空已结束
      </button>
    </header>

    <section class="panel">
      <div class="filter-bar" role="tablist" aria-label="任务状态">
        <button
          v-for="item in filters"
          :key="item.id"
          type="button"
          class="filter-chip"
          :class="{ active: statusFilter === item.id }"
          role="tab"
          :aria-selected="statusFilter === item.id"
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
                <template v-if="countText(task)">
                  <span class="dot">·</span>
                  <span class="count-text">{{ countText(task) }}</span>
                </template>
                <template v-if="isEnded(task.status) && durationText(task)">
                  <span class="dot">·</span>
                  <span class="duration-text">耗时 {{ durationText(task) }}</span>
                </template>
              </p>
              <p
                v-if="task.status === 'error' || task.status === 'cancelled'"
                class="task-reason"
                :class="task.status === 'error' ? 'is-error' : 'is-cancelled'"
                :title="taskReason(task)"
              >
                {{ task.status === 'error' ? '失败原因' : '取消原因' }}：{{ taskReason(task) }}
              </p>
            </div>
            <div class="file-actions">
              <span class="status" :class="`status-${task.status}`">
                {{ statusLabel(task.status) }}
                <template v-if="task.status === 'running'"> {{ task.progress }}%</template>
              </span>
              <button
                v-if="task.status === 'done' && outputTarget(task)"
                type="button"
                class="btn btn-soft btn-sm"
                @click="onReveal(task)"
              >
                打开目录
              </button>
              <button
                v-if="isActive(task.status)"
                type="button"
                class="btn btn-danger-ghost btn-sm"
                @click="onCancel(task)"
              >
                取消
              </button>
              <button
                type="button"
                class="btn btn-text btn-sm"
                @click="onDelete(task)"
              >
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

            <p class="task-foot">{{ footerText(task) }}</p>
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
  height: 30px;
  padding: 0 11px;
  border-radius: 8px;
  border: 1px solid var(--line);
  background: rgba(8, 16, 15, 0.35);
  color: var(--text-muted);
  font-size: 0.8rem;
  font-weight: 600;
  line-height: 1;
  transition:
    background 0.14s ease,
    border-color 0.14s ease,
    color 0.14s ease;
}

.filter-chip:hover {
  color: var(--text);
  border-color: var(--line-strong);
  background: rgba(255, 255, 255, 0.04);
}

.filter-chip.active {
  color: var(--accent);
  border-color: rgba(94, 234, 212, 0.36);
  background: rgba(94, 234, 212, 0.12);
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
  align-items: start;
}

.type-tag {
  color: var(--accent);
  font-weight: 600;
}

.dot {
  margin: 0 0.35em;
  opacity: 0.45;
}

.count-text {
  font-variant-numeric: tabular-nums;
  color: var(--text-muted);
}

.duration-text {
  color: var(--ok);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.task-reason {
  margin: 6px 0 0;
  font-size: 0.78rem;
  line-height: 1.35;
  word-break: break-word;
}

.task-reason.is-error {
  color: var(--danger);
}

.task-reason.is-cancelled {
  color: var(--text-muted);
}

.task-foot {
  grid-column: 1 / -1;
  margin: 2px 0 0;
  color: var(--text-dim);
  font-size: 0.76rem;
}
</style>
