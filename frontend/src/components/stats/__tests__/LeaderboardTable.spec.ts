import { createApp, h, nextTick } from 'vue'
import { afterEach, describe, expect, it } from 'vitest'
import type { LeaderboardItem } from '@/api/admin'
import LeaderboardTable from '../LeaderboardTable.vue'

let app: ReturnType<typeof createApp> | null = null

afterEach(() => {
  app?.unmount()
  app = null
})

describe('LeaderboardTable selection', () => {
  it('emits the selected item for mouse and keyboard activation', async () => {
    const item: LeaderboardItem = {
      rank: 1,
      id: 'user-1',
      name: 'alice',
      value: 10,
      requests: 10,
      tokens: 100,
      cost: 1,
    }
    const selectedIds: string[] = []
    const root = document.createElement('div')
    document.body.appendChild(root)

    app = createApp({
      render: () => h(LeaderboardTable, {
        title: 'Users',
        items: [item],
        metric: 'requests',
        selectable: true,
        selectedItemId: item.id,
        showMetricSelect: false,
        onSelectItem: (selected: LeaderboardItem) => selectedIds.push(selected.id),
      }),
    })
    app.mount(root)
    await nextTick()

    const row = root.querySelector<HTMLTableRowElement>('tbody tr')
    expect(row?.dataset.state).toBe('selected')
    expect(row?.tabIndex).toBe(0)

    row?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    row?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    row?.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }))
    await nextTick()

    expect(selectedIds).toEqual(['user-1', 'user-1', 'user-1'])
    root.remove()
  })
})
