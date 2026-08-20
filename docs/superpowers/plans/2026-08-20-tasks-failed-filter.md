# Tasks Failed Filter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 任务列表 chip 栏增加「失败」，只展示 `status === 'error'` 的任务。

**Architecture:** 仅改 `TasksView.vue` 的本地 `StatusFilter` / `filters` / `filteredTasks` / `filterCounts`，不改 `taskStore`。

**Tech Stack:** Vue 3 + Vitest + `@vue/test-utils`

## Global Constraints

- 「失败」仅匹配 `status === 'error'`；`cancelled` 不计入
- 「已结束」语义不变（仍含 `done` | `error` | `cancelled`）
- chip 顺序：`全部 | 进行中 | 已结束 | 失败`

---

### Task 1: 失败筛选 chip

**Files:**
- Modify: `src/views/TasksView.vue`
- Test: `src/views/TasksView.test.ts`

**Interfaces:**
- Consumes: `tasks`、`AppTask['status']`（现有）
- Produces: `StatusFilter` 含 `'failed'`；`filters` 含 `{ id: 'failed', label: '失败' }`

- [ ] **Step 1: Write the failing test**

在 `TasksView.test.ts` 新增：

```ts
describe('task status filter', () => {
  it('shows only error tasks when the failed filter is selected', async () => {
    tasks.value = [
      makeTask('done', '完成', 'done', { videoCount: 1, doneCount: 1 }),
      makeTask('error', '失败', 'error', { videoCount: 1, doneCount: 0 }, '编码失败'),
      makeTask('cancelled', '取消', 'cancelled', { videoCount: 1, doneCount: 0 }, '用户取消'),
      makeTask('running', '进行中', 'running', { videoCount: 1, doneCount: 0 }),
    ]

    const wrapper = mount(TasksView)
    const failedChip = wrapper
      .findAll('button')
      .find((button) => button.text().includes('失败') && !button.text().includes('错误详情'))
    expect(failedChip).toBeTruthy()
    expect(failedChip!.text()).toContain('1')

    await failedChip!.trigger('click')

    expect(wrapper.text()).toContain('失败')
    expect(wrapper.text()).toContain('编码失败')
    expect(wrapper.text()).not.toContain('完成')
    expect(wrapper.text()).not.toContain('用户取消')
    expect(wrapper.findAll('.task-item')).toHaveLength(1)
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm exec vitest run src/views/TasksView.test.ts`

Expected: FAIL（无「失败」筛选 chip，或点击后仍显示多条）

- [ ] **Step 3: Write minimal implementation**

在 `TasksView.vue`：

```ts
type StatusFilter = 'all' | 'active' | 'ended' | 'failed'

const filters: { id: StatusFilter; label: string }[] = [
  { id: 'all', label: '全部' },
  { id: 'active', label: '进行中' },
  { id: 'ended', label: '已结束' },
  { id: 'failed', label: '失败' },
]

function isFailed(status: TaskStatus) {
  return status === 'error'
}

const filteredTasks = computed(() => {
  if (statusFilter.value === 'all') return tasks.value
  if (statusFilter.value === 'active') return tasks.value.filter((t) => isActive(t.status))
  if (statusFilter.value === 'failed') return tasks.value.filter((t) => isFailed(t.status))
  return tasks.value.filter((t) => isEnded(t.status))
})

const filterCounts = computed(() => {
  let active = 0
  let ended = 0
  let failed = 0
  for (const t of tasks.value) {
    if (isActive(t.status)) active += 1
    else if (isEnded(t.status)) ended += 1
    if (isFailed(t.status)) failed += 1
  }
  return { all: tasks.value.length, active, ended, failed }
})
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm exec vitest run src/views/TasksView.test.ts`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/views/TasksView.vue src/views/TasksView.test.ts
git commit -m "任务列表增加失败状态筛选"
```
