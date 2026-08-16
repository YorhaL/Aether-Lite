import { describe, expect, it } from 'vitest'

import { normalizeChatPiiRedactionProviderConfig } from '@/api/endpoints/types'

describe('normalizeChatPiiRedactionProviderConfig', () => {
  it('defaults unsupported payloads to disabled', () => {
    expect(normalizeChatPiiRedactionProviderConfig(null)).toEqual({ enabled: false })
    expect(normalizeChatPiiRedactionProviderConfig({})).toEqual({ enabled: false })
    expect(normalizeChatPiiRedactionProviderConfig({ enabled: 'yes' })).toEqual({ enabled: false })
  })

  it('passes through enabled state only', () => {
    expect(normalizeChatPiiRedactionProviderConfig({ enabled: true })).toEqual({ enabled: true })
    expect(normalizeChatPiiRedactionProviderConfig({ enabled: false, entities: ['email'] })).toEqual({ enabled: false })
  })
})
