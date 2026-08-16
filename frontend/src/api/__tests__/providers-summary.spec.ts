import { beforeEach, describe, expect, it, vi } from 'vitest'

const { getMock } = vi.hoisted(() => ({ getMock: vi.fn() }))

vi.mock('@/api/client', () => ({
  default: {
    get: getMock,
  },
}))

import { getProvider, getProviderMappingPreview, getProvidersSummary } from '@/api/endpoints/providers'

const provider = {
  id: 'provider-1',
  name: 'Provider 1',
  provider_type: 'custom',
  is_active: true,
  endpoints: [],
}

describe('getProvidersSummary', () => {
  beforeEach(() => {
    getMock.mockReset()
  })

  it('normalizes the paginated summary response', async () => {
    getMock.mockResolvedValue({
      data: { total: 1, page: 1, page_size: 20, items: [provider] },
    })

    const result = await getProvidersSummary({ page: 1, page_size: 20, search: 'paged' })

    expect(result.total).toBe(1)
    expect(result.items).toHaveLength(1)
  })

  it('supports the legacy array response without reading an undefined items field', async () => {
    getMock.mockResolvedValue({ data: [provider] })

    const result = await getProvidersSummary({ page: 2, page_size: 20, search: 'legacy' })

    expect(result).toMatchObject({ total: 1, page: 2, page_size: 20 })
    expect(result.items).toHaveLength(1)
  })
})

describe('getProvider', () => {
  beforeEach(() => {
    getMock.mockReset()
  })

  it('deduplicates concurrent detail requests without caching the settled result', async () => {
    let resolveRequest: ((value: { data: typeof provider }) => void) | undefined
    getMock.mockImplementation(() => new Promise((resolve) => {
      resolveRequest = resolve
    }))

    const firstRequest = getProvider('provider-1')
    const secondRequest = getProvider('provider-1')

    expect(getMock).toHaveBeenCalledTimes(1)
    resolveRequest?.({ data: provider })
    await expect(Promise.all([firstRequest, secondRequest])).resolves.toHaveLength(2)

    getMock.mockResolvedValue({ data: provider })
    await getProvider('provider-1')
    expect(getMock).toHaveBeenCalledTimes(2)
  })
})

describe('getProviderMappingPreview', () => {
  beforeEach(() => {
    getMock.mockReset()
  })

  it('normalizes a non-contract response instead of exposing missing arrays to the UI', async () => {
    getMock.mockResolvedValue({
      data: { message: '演示模式：该接口暂未模拟', demo_mode: true },
    })

    const result = await getProviderMappingPreview('provider-demo')

    expect(result).toEqual({
      provider_id: 'provider-demo',
      provider_name: '',
      keys: [],
      total_keys: 0,
      total_matches: 0,
      truncated: false,
      truncated_keys: 0,
      truncated_models: 0,
    })
  })

  it('normalizes missing nested mapping arrays', async () => {
    getMock.mockResolvedValue({
      data: {
        provider_id: 'provider-nested',
        provider_name: 'Nested Provider',
        keys: [{
          key_id: 'key-1',
          key_name: 'Primary',
          masked_key: 'sk-***',
          is_active: true,
          allowed_models: null,
          matching_global_models: [{
            global_model_id: 'model-1',
            global_model_name: 'gpt-5',
            display_name: 'GPT-5',
            is_active: true,
            matched_models: null,
          }],
        }],
      },
    })

    const result = await getProviderMappingPreview('provider-nested')

    expect(result.keys[0]?.allowed_models).toEqual([])
    expect(result.keys[0]?.matching_global_models[0]?.matched_models).toEqual([])
    expect(result.total_keys).toBe(1)
    expect(result.total_matches).toBe(1)
  })
})
