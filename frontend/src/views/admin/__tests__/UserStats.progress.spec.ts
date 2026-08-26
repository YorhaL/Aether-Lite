import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

const source = readFileSync(
  resolve(process.cwd(), 'src/views/admin/UserStats.vue'),
  'utf8',
)

describe('UserStats usage progress bars', () => {
  it('uses a valid theme color while preserving the calculated model width', () => {
    expect(source).toContain('class="h-full rounded-full bg-primary"')
    expect(source).toContain(':style="{ width: usageShare(item.total_tokens) }"')
    expect(source).not.toContain('bg-primary/75')
  })
})
