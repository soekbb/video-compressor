// @vitest-environment jsdom

import { mount } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import { tasks, type AppTask, type TaskMeta } from '../taskStore'
import TasksView from './TasksView.vue'

const timestamp = '2026-08-07T10:00:00.000Z'

function makeTask(
  id: string,
  title: string,
  status: AppTask['status'],
  meta: TaskMeta,
  error?: string,
): AppTask {
  return {
    id,
    type: 'batch',
    title,
    status,
    progress: status === 'done' ? 100 : 50,
    error,
    meta: JSON.stringify(meta),
    createdAt: timestamp,
    updatedAt: timestamp,
  }
}

afterEach(() => {
  tasks.value = []
})

describe('task failure details', () => {
  it('keeps every failed input hidden until the failed task is expanded', async () => {
    tasks.value = [
      makeTask(
        'failed',
        '失败批次',
        'error',
        {
          videoCount: 3,
          doneCount: 1,
          failures: [
            {
              inputPath: '/input/first broken.mp4',
              message: 'first final ffmpeg error',
            },
            {
              inputPath: 'C:\\input\\second-broken.mov',
              message: 'second final ffmpeg error',
            },
          ],
        },
        '部分文件压制失败',
      ),
      makeTask('skipped', '仅跳过批次', 'done', {
        videoCount: 2,
        doneCount: 2,
        completedCount: 0,
        skippedCount: 2,
        failures: [],
      }),
    ]

    const wrapper = mount(TasksView)

    expect(wrapper.text()).toContain('失败原因：部分文件压制失败')
    expect(wrapper.text()).not.toContain('/input/first broken.mp4')
    expect(wrapper.text()).not.toContain('first final ffmpeg error')

    const detailButtons = wrapper
      .findAll('button')
      .filter((button) => button.text().includes('错误详情'))
    expect(detailButtons).toHaveLength(1)

    await detailButtons[0]!.trigger('click')

    expect(wrapper.text()).toContain('first broken.mp4')
    expect(wrapper.text()).toContain('/input/first broken.mp4')
    expect(wrapper.text()).toContain('first final ffmpeg error')
    expect(wrapper.text()).toContain('second-broken.mov')
    expect(wrapper.text()).toContain('C:\\input\\second-broken.mov')
    expect(wrapper.text()).toContain('second final ffmpeg error')

    await detailButtons[0]!.trigger('click')
    expect(wrapper.text()).not.toContain('/input/first broken.mp4')
  })
})
