import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

const source = readFileSync(
  resolve(process.cwd(), 'src/views/admin/UserStats.vue'),
  'utf8',
)

describe('UserStats usage progress bars', () => {
  it('uses the same green color while preserving the calculated width', () => {
    expect(source.match(/class="h-full rounded-full bg-emerald-500\/75"/g)).toHaveLength(2)
    expect(source).toContain(':style="{ width: usageShare(item.total_tokens) }"')
    expect(source).not.toContain('class="h-full rounded-full bg-primary"')
  })
})
