import type { GlobalModelCreate, GlobalModelResponse, GlobalModelUpdate } from '@/api/global-models'
import type { TieredPricingConfig } from '@/api/endpoints/types'
import type { ModelsDevModelItem } from '@/api/models-dev'

export const EMBEDDING_API_FORMATS = [
  'openai:embedding',
  'gemini:embedding',
] as const

export const RERANK_API_FORMATS = [
  'openai:rerank',
] as const

export interface GlobalModelFormPayloadState {
  name: string
  display_name: string
  default_price_per_request?: number
  supported_capabilities?: string[]
  config?: Record<string, unknown>
  is_active?: boolean
}

export function cloneTieredPricingConfig(
  pricing: TieredPricingConfig,
): TieredPricingConfig {
  // Presets live inside a Vue ref and can therefore be reactive proxies, which
  // structuredClone cannot clone directly. Pricing configs are JSON payloads.
  return JSON.parse(JSON.stringify(pricing)) as TieredPricingConfig
}

export function findGlobalModelByName<T extends { name: string }>(
  models: T[],
  modelName: string,
): T | undefined {
  const normalizedName = modelName.trim().toLowerCase()
  return models.find(model => model.name.trim().toLowerCase() === normalizedName)
}

function normalizeJsonValue(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(normalizeJsonValue)
  }
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>)
        .filter(([, entryValue]) => entryValue !== undefined)
        .sort(([leftKey], [rightKey]) => leftKey.localeCompare(rightKey))
        .map(([key, entryValue]) => [key, normalizeJsonValue(entryValue)]),
    )
  }
  return value
}

export function tieredPricingConfigsEqual(
  currentPricing: TieredPricingConfig | null | undefined,
  onlinePricing: TieredPricingConfig | null | undefined,
): boolean {
  return JSON.stringify(normalizeJsonValue(currentPricing ?? null))
    === JSON.stringify(normalizeJsonValue(onlinePricing ?? null))
}

export interface GlobalModelPriceSyncEntry {
  model: GlobalModelResponse
  onlineModel: ModelsDevModelItem
}

export interface GlobalModelPriceSyncPlan {
  syncable: GlobalModelPriceSyncEntry[]
  unchanged: GlobalModelPriceSyncEntry[]
  unsupported: GlobalModelPriceSyncEntry[]
  unavailable: GlobalModelResponse[]
}

export function buildGlobalModelPriceSyncPlan(
  models: GlobalModelResponse[],
  onlineModels: ModelsDevModelItem[],
  pricingProviderIds?: ReadonlyMap<string, string>,
): GlobalModelPriceSyncPlan {
  const onlineModelsByName = new Map<string, ModelsDevModelItem>()
  const onlineModelsBySource = new Map<string, ModelsDevModelItem>()
  for (const onlineModel of onlineModels) {
    const normalizedName = onlineModel.modelId.trim().toLowerCase()
    if (!onlineModelsByName.has(normalizedName)) {
      onlineModelsByName.set(normalizedName, onlineModel)
    }
    onlineModelsBySource.set(
      `${onlineModel.providerId.trim().toLowerCase()}\u0000${normalizedName}`,
      onlineModel,
    )
  }

  const plan: GlobalModelPriceSyncPlan = {
    syncable: [],
    unchanged: [],
    unsupported: [],
    unavailable: [],
  }
  for (const model of models) {
    const normalizedName = model.name.trim().toLowerCase()
    const sourceProviderId = pricingProviderIds?.get(model.id)?.trim().toLowerCase()
    const onlineModel = pricingProviderIds
      ? sourceProviderId
        ? onlineModelsBySource.get(`${sourceProviderId}\u0000${normalizedName}`)
        : undefined
      : onlineModelsByName.get(normalizedName)
    if (onlineModel?.pricingUnsupportedFields?.length) {
      plan.unsupported.push({ model, onlineModel })
    } else if (!onlineModel?.tieredPricing) {
      plan.unavailable.push(model)
    } else if (tieredPricingConfigsEqual(model.default_tiered_pricing, onlineModel.tieredPricing)) {
      plan.unchanged.push({ model, onlineModel })
    } else {
      plan.syncable.push({ model, onlineModel })
    }
  }
  return plan
}

function cleanGlobalModelConfig(form: GlobalModelFormPayloadState): Record<string, unknown> | undefined {
  return form.config && Object.keys(form.config).length > 0 ? form.config : undefined
}

export function buildGlobalModelCreatePayload(
  form: GlobalModelFormPayloadState,
  defaultTieredPricing: TieredPricingConfig,
): GlobalModelCreate {
  return {
    name: form.name ?? '',
    display_name: form.display_name ?? '',
    config: cleanGlobalModelConfig(form),
    default_price_per_request: form.default_price_per_request ?? undefined,
    default_tiered_pricing: defaultTieredPricing,
    supported_capabilities: form.supported_capabilities?.length ? form.supported_capabilities : undefined,
    is_active: form.is_active,
  }
}

export function buildGlobalModelUpdatePayload(
  form: GlobalModelFormPayloadState,
  defaultTieredPricing: TieredPricingConfig,
): GlobalModelUpdate {
  return {
    display_name: form.display_name,
    config: cleanGlobalModelConfig(form) || null,
    default_price_per_request: form.default_price_per_request ?? null,
    default_tiered_pricing: defaultTieredPricing,
    supported_capabilities: form.supported_capabilities?.length ? form.supported_capabilities : null,
    is_active: form.is_active,
  }
}
