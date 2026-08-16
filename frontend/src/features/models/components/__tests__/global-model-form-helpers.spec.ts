import { describe, expect, it } from 'vitest'
import { reactive } from 'vue'

import {
  EMBEDDING_API_FORMATS,
  buildGlobalModelPriceSyncPlan,
  buildGlobalModelCreatePayload,
  buildGlobalModelUpdatePayload,
  cloneTieredPricingConfig,
  findGlobalModelByName,
  tieredPricingConfigsEqual,
} from '../global-model-form-helpers'
import type { TieredPricingConfig } from '@/api/endpoints/types'

const embeddingPricing = {
  tiers: [{ up_to: null, input_price_per_1m: 0.02, output_price_per_1m: 0 }],
}

describe('global model form embedding payload helpers', () => {
  it('preserves embedding metadata in create payloads', () => {
    const payload = buildGlobalModelCreatePayload({
      name: 'text-embedding-3-small',
      display_name: 'text-embedding-3-small',
      supported_capabilities: ['embedding'],
      config: {
        streaming: false,
        embedding: true,
        model_type: 'embedding',
        api_formats: [...EMBEDDING_API_FORMATS],
      },
      is_active: true,
    }, embeddingPricing)

    expect(payload).toMatchObject({
      name: 'text-embedding-3-small',
      supported_capabilities: ['embedding'],
      config: {
        streaming: false,
        embedding: true,
        model_type: 'embedding',
        api_formats: [
          'openai:embedding',
          'gemini:embedding',
        ],
      },
    })
  })

  it('preserves embedding metadata in update payloads', () => {
    const payload = buildGlobalModelUpdatePayload({
      name: 'unused-on-update',
      display_name: 'Embedding Model',
      supported_capabilities: ['embedding'],
      config: {
        streaming: false,
        embedding: true,
        model_type: 'embedding',
        api_formats: ['gemini:embedding'],
      },
      is_active: true,
    }, embeddingPricing)

    expect(payload.supported_capabilities).toEqual(['embedding'])
    expect(payload.config).toEqual({
      streaming: false,
      embedding: true,
      model_type: 'embedding',
      api_formats: ['gemini:embedding'],
    })
  })
})

describe('global model form pricing presets', () => {
  it('clones reactive pricing before opening the preset editor', () => {
    const pricing = reactive({
      tiers: [{
        up_to: null,
        input_price_per_1m: 3,
        output_price_per_1m: 15,
      }],
    }) as TieredPricingConfig

    const cloned = cloneTieredPricingConfig(pricing)

    expect(cloned).toEqual(pricing)
    expect(cloned).not.toBe(pricing)
    expect(cloned.tiers[0]).not.toBe(pricing.tiers[0])

    cloned.tiers[0].input_price_per_1m = 9
    expect(pricing.tiers[0].input_price_per_1m).toBe(3)
  })

  it('matches existing models by normalized model ID', () => {
    const existingModel = { id: 'model-1', name: ' Claude-Sonnet-5 ' }

    expect(findGlobalModelByName([existingModel], 'claude-sonnet-5')).toBe(existingModel)
    expect(findGlobalModelByName([existingModel], 'claude-opus-5')).toBeUndefined()
  })

  it('compares pricing independently of object key order', () => {
    const currentPricing = {
      processing_tiers: {
        priority: { price_multiplier: 2 },
      },
      tiers: [{
        output_price_per_1m: 15,
        input_price_per_1m: 3,
        up_to: null,
      }],
    } as TieredPricingConfig
    const onlinePricing = {
      tiers: [{
        up_to: null,
        input_price_per_1m: 3,
        output_price_per_1m: 15,
      }],
      processing_tiers: {
        priority: { price_multiplier: 2 },
      },
    } as TieredPricingConfig

    expect(tieredPricingConfigsEqual(currentPricing, onlinePricing)).toBe(true)
    onlinePricing.tiers[0].output_price_per_1m = 16
    expect(tieredPricingConfigsEqual(currentPricing, onlinePricing)).toBe(false)
  })

  it('groups models by their selected provider pricing sync state', () => {
    const makeGlobalModel = (id: string, name: string, inputPrice: number) => ({
      id,
      name,
      display_name: name,
      is_active: true,
      default_tiered_pricing: {
        tiers: [{ up_to: null, input_price_per_1m: inputPrice, output_price_per_1m: 10 }],
      },
      created_at: '2026-07-23T00:00:00Z',
    })
    const makeOnlineModel = (modelId: string, inputPrice?: number) => ({
      providerId: 'anthropic',
      providerName: 'Anthropic',
      modelId,
      modelName: modelId,
      tieredPricing: inputPrice === undefined
        ? undefined
        : { tiers: [{ up_to: null, input_price_per_1m: inputPrice, output_price_per_1m: 10 }] },
    })
    const currentModel = makeGlobalModel('current', 'current-model', 2)
    const staleModel = makeGlobalModel('stale', 'stale-model', 3)
    const unavailableModel = makeGlobalModel('missing', 'missing-model', 4)
    const unsupportedModel = makeGlobalModel('unsupported', 'unsupported-model', 5)

    const plan = buildGlobalModelPriceSyncPlan(
      [currentModel, staleModel, unavailableModel, unsupportedModel],
      [
        makeOnlineModel('current-model', 2),
        makeOnlineModel('stale-model', 5),
        {
          ...makeOnlineModel('unsupported-model'),
          pricingUnsupportedFields: ['reasoning'],
        },
      ],
    )

    expect(plan.unchanged.map(entry => entry.model.id)).toEqual(['current'])
    expect(plan.syncable.map(entry => entry.model.id)).toEqual(['stale'])
    expect(plan.unsupported.map(entry => entry.model.id)).toEqual(['unsupported'])
    expect(plan.unavailable.map(model => model.id)).toEqual(['missing'])
  })

  it('resolves each model from its remembered pricing provider', () => {
    const makeGlobalModel = (id: string, name: string) => ({
      id,
      name,
      display_name: name,
      is_active: true,
      default_tiered_pricing: {
        tiers: [{ up_to: null, input_price_per_1m: 1, output_price_per_1m: 10 }],
      },
      created_at: '2026-07-23T00:00:00Z',
    })
    const makeOnlineModel = (providerId: string, modelId: string, inputPrice: number) => ({
      providerId,
      providerName: providerId,
      modelId,
      modelName: modelId,
      tieredPricing: {
        tiers: [{ up_to: null, input_price_per_1m: inputPrice, output_price_per_1m: 10 }],
      },
    })
    const openAiModel = makeGlobalModel('openai-model', 'shared-model')
    const anthropicModel = makeGlobalModel('anthropic-model', 'other-model')
    const missingSourceModel = makeGlobalModel('missing-source', 'shared-model')

    const plan = buildGlobalModelPriceSyncPlan(
      [openAiModel, anthropicModel, missingSourceModel],
      [
        makeOnlineModel('anthropic', 'shared-model', 2),
        makeOnlineModel('openai', 'shared-model', 3),
        makeOnlineModel('anthropic', 'other-model', 4),
        makeOnlineModel('openai', 'other-model', 5),
      ],
      new Map([
        [openAiModel.id, 'openai'],
        [anthropicModel.id, 'anthropic'],
      ]),
    )

    expect(plan.syncable.map(entry => [
      entry.model.id,
      entry.onlineModel.providerId,
      entry.onlineModel.tieredPricing?.tiers[0].input_price_per_1m,
    ])).toEqual([
      ['openai-model', 'openai', 3],
      ['anthropic-model', 'anthropic', 4],
    ])
    expect(plan.unavailable.map(model => model.id)).toEqual(['missing-source'])
  })

})
