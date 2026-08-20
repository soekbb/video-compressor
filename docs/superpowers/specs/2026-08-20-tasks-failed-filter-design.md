# 任务列表：失败筛选

## 目标

在任务页现有状态 chip 栏增加「失败」，只展示 `status === 'error'` 的任务，便于从大量已结束记录中快速定位真正失败的项。

## 范围

- **改动文件**：`src/views/TasksView.vue`、`src/views/TasksView.test.ts`
- **不改**：`taskStore` 状态模型、清空已结束、取消/删除行为、「已结束」语义

## 行为

| Chip | 匹配 |
|------|------|
| 全部 | 全部任务 |
| 进行中 | `running` \| `pending` |
| 已结束 | `done` \| `error` \| `cancelled`（不变） |
| 失败 | 仅 `error` |

- 失败计数 = 当前任务列表中 `status === 'error'` 的数量
- 该筛选下无任务时，沿用空态文案：「该状态下暂无任务」
- 「已取消」仍只出现在「全部 / 已结束」，不计入「失败」

## UI

chip 顺序：`全部 | 进行中 | 已结束 | 失败`。样式、交互与现有 filter-chip 一致（含数量角标）。

## 测试

- 构造含 `done` / `error` / `cancelled` / `running` 的任务列表
- 选择「失败」后列表只含 `error` 任务，且失败 chip 计数正确
