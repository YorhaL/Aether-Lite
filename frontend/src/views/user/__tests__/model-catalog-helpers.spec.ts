import { describe, expect, it } from 'vitest'

import type { PublicGlobalModel } from '@/api/public-models'
import { getModelCapabilityLabels, supportsEmbedding, supportsRerank } from '../model-catalog-helpers'

function model(overrides: Partial<PublicGlobalModel>): PublicGlobalModel {
  return {
    id: 'gm-test',
    name: 'model-test',
    display_name: 'Model Test',
    is_active: true,
    default_tiered_pricing: null,
    default_price_per_request: null,
    supported_capabilities: null,
    config: null,
    usage_count: 0,
    ...overrides,
  }
}

describe('model catalog embedding helpers', () => {
  it('labels embedding models distinctly from chat models', () => {
    expect(getModelCapabilityLabels(model({
      supported_capabilities: ['embedding'],
      config: { streaming: false, api_formats: ['openai:embedding'] },
    }))).toEqual(['Embedding'])

    expect(getModelCapabilityLabels(model({
      config: { streaming: true },
    }))).toEqual(['Chat'])
  })

  it('detects embedding metadata from explicit and config-derived frontend fields', () => {
    expect(supportsEmbedding(model({ supports_embedding: true }))).toBe(true)
    expect(supportsEmbedding(model({ config: { embedding: true } }))).toBe(true)
    expect(supportsEmbedding(model({ config: { model_type: 'embedding' } }))).toBe(true)
    expect(supportsEmbedding(model({ config: { api_formats: ['openai:embedding'] } }))).toBe(true)
    expect(supportsEmbedding(model({ config: { api_formats: ['gemini:embedding'] } }))).toBe(true)
    expect(supportsEmbedding(model({ config: { api_formats: ['openai:chat'] } }))).toBe(false)
  })

  it('labels rerank models distinctly from chat and embedding models', () => {
    const rerank = model({
      supported_capabilities: ['rerank'],
      config: { streaming: false, api_formats: ['openai:rerank'] },
    })

    expect(supportsRerank(rerank)).toBe(true)
    expect(getModelCapabilityLabels(rerank)).toEqual(['Rerank'])
    expect(supportsRerank(model({ config: { api_formats: ['openai:embedding'] } }))).toBe(false)
  })
})
