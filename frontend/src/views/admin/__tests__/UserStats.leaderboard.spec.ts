import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

function readSource(path: string): string {
  return readFileSync(resolve(process.cwd(), path), 'utf8')
}

describe('UserStats leaderboard interactions', () => {
  it('renders user group badges next to leaderboard names', () => {
    const source = readSource('src/views/admin/UserStats.vue')

    expect(source).toContain('<template #name="{ item }">')
    expect(source).toContain('leaderboardUserGroups(item.id)')
    expect(source).toContain('variant="outline"')
    expect(source).toContain('{{ group.name }}')
    expect(source).toContain('usersApi.getAllUsersPage({ skip, limit: pageSize })')
    expect(source).toContain('page.has_more')
  })

  it('selects a leaderboard user for the summary', () => {
    const pageSource = readSource('src/views/admin/UserStats.vue')
    const tableSource = readSource('src/components/stats/LeaderboardTable.vue')

    expect(pageSource).toContain('@select-item="selectLeaderboardUser"')
    expect(pageSource).toContain('selectedUserId.value = item.id')
    expect(tableSource).toContain("emit('select-item', item)")
    expect(tableSource).toContain('@keydown.enter.prevent="selectItem(item)"')
    expect(tableSource).toContain('@keydown.space.prevent="selectItem(item)"')
  })
})
