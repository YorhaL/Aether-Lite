import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

function readSource(path: string): string {
  return readFileSync(resolve(process.cwd(), path), 'utf8')
}

describe('user management layout', () => {
  it('separates statistics and traffic control policy on desktop', () => {
    const listSource = readSource('src/features/users/components/UserManagementList.vue')
    const rowSource = readSource('src/features/users/components/UserTableRow.vue')

    expect(listSource).toContain("legacyT('统计')")
    expect(listSource).toContain("legacyT('流控策略')")
    expect(listSource).not.toContain("legacyT('统计/限速')")

    const statisticsCell = rowSource.split("legacyT('请求:')")[1]?.split('</TableCell>')[0]
    const policyCell = rowSource.split('RPM:')[1]?.split('</TableCell>')[0]

    expect(statisticsCell).toContain('row.tokensLabel')
    expect(statisticsCell).not.toContain('row.rateLimitLabel')
    expect(policyCell).toContain('row.rateLimitLabel')
    expect(policyCell).toContain('row.dailyUsageLimitLabel')
    expect(policyCell).toContain('row.concurrentLimitLabel')
    expect(policyCell).not.toContain('{{ legacyT(row.rateLimitSource) }}')
    expect(policyCell).not.toContain('{{ legacyT(row.dailyUsageLimitSource) }}')
    expect(policyCell).not.toContain('{{ legacyT(row.concurrentLimitSource) }}')
  })

  it('shows policy in its own mobile section instead of status badges', () => {
    const cardSource = readSource('src/features/users/components/UserMobileCard.vue')
    const statusSource = readSource('src/features/users/components/UserStatusBadges.vue')

    expect(cardSource).toContain("legacyT('统计')")
    expect(cardSource).toContain("legacyT('流控策略')")
    expect(cardSource).toContain('row.rateLimitLabel')
    expect(cardSource).toContain('row.dailyUsageLimitLabel')
    expect(cardSource).toContain('row.concurrentLimitLabel')
    expect(cardSource).not.toContain('{{ legacyT(row.rateLimitSource) }}')
    expect(cardSource).not.toContain('{{ legacyT(row.dailyUsageLimitSource) }}')
    expect(cardSource).not.toContain('{{ legacyT(row.concurrentLimitSource) }}')
    expect(statusSource).not.toContain('row.rateLimitLabel')
    expect(statusSource).not.toContain('row.dailyUsageLimitLabel')
  })
})
