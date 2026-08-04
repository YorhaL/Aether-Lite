import { describe, expect, it } from 'vitest'

import {
  formatUserEffectiveRateLimitSource,
  resolveUserEffectiveRateLimit,
} from '../rateLimitPolicy'

describe('user effective RPM policy', () => {
  it('uses the effective policy value instead of the legacy user field', () => {
    expect(resolveUserEffectiveRateLimit({
      rate_limit: 10,
      effective_policy: {
        rate_limit: {
          mode: 'custom',
          value: 100,
          source: 'group',
          group_name: 'Plan RPM',
        },
      },
    })).toBe(100)
  })

  it('preserves a system fallback null instead of reviving a legacy value', () => {
    expect(resolveUserEffectiveRateLimit({
      rate_limit: 10,
      effective_policy: {
        rate_limit: {
          mode: 'system',
          value: null,
          source: 'fallback',
        },
      },
    })).toBeNull()
  })

  it('falls back to the legacy value for older servers', () => {
    expect(resolveUserEffectiveRateLimit({ rate_limit: 30 })).toBe(30)
  })

  it('describes combined policies as inheritance from multiple groups', () => {
    const translations: Record<string, string> = {
      '继承自多个分组：': 'Inherited from groups: ',
      '继承自多个分组': 'Inherited from groups',
    }
    const label = formatUserEffectiveRateLimitSource({
      rate_limit: null,
      effective_policy: {
        rate_limit: {
          mode: 'custom',
          value: 100,
          source: 'combined',
          group_names: ['Basic', 'Pro'],
        },
      },
    }, (key) => translations[key] ?? key, 'en-US')

    expect(label).toBe('Inherited from groups: Basic, Pro')
  })
})
