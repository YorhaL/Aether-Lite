import client from '../client'
import { buildCacheKey, cachedRequest, dedupedRequest } from '@/utils/cache'
import type {
  FailoverRulesConfig,
  ProviderConfig,
  ProviderWithEndpointsSummary,
} from './types'
import { normalizeChatPiiRedactionProviderConfig as normalizeChatPiiRedactionProvider } from './types'

interface ProviderRequestOptions {
  timeout?: number
}

interface ProviderReadOptions {
  timeout?: number
  cacheTtlMs?: number
}

/**
 * 获取 Providers 摘要（分页）
 */
export interface ProviderSummaryQuery {
  page?: number
  page_size?: number
  search?: string
  status?: string
  api_format?: string
  model_id?: string
}

export interface ProviderSummaryPageResponse {
  total: number
  page: number
  page_size: number
  items: ProviderWithEndpointsSummary[]
}

type ProviderSummaryResponse = ProviderSummaryPageResponse | ProviderWithEndpointsSummary[]

function normalizeProviderSummary(
  provider: ProviderWithEndpointsSummary,
): ProviderWithEndpointsSummary {
  return {
    ...provider,
    chat_pii_redaction: normalizeChatPiiRedactionProvider(provider.chat_pii_redaction),
    responses_websocket_enabled: provider.responses_websocket_enabled === true,
  }
}

export async function getProvidersSummary(
  params: ProviderSummaryQuery = {},
  options: ProviderReadOptions = {},
): Promise<ProviderSummaryPageResponse> {
  const cacheTtlMs = options.cacheTtlMs ?? 0
  const cacheKey = buildCacheKey('providers:summary', params as Record<string, unknown>)
  return cachedRequest(
    cacheKey,
    async () => {
      const response = await client.get<ProviderSummaryResponse>(
        '/api/admin/providers/summary',
        {
          params,
          timeout: options.timeout,
        },
      )
      const data = response.data
      if (Array.isArray(data)) {
        return {
          total: data.length,
          page: params.page ?? 1,
          page_size: params.page_size ?? data.length,
          items: data.map(normalizeProviderSummary),
        }
      }

      return {
        ...data,
        items: (data.items ?? []).map(normalizeProviderSummary),
      }
    },
    cacheTtlMs,
  )
}

/**
 * 获取单个 Provider 的详细信息
 */
export async function getProvider(providerId: string): Promise<ProviderWithEndpointsSummary> {
  return dedupedRequest(`providers:detail:${providerId}`, async () => {
    const response = await client.get<ProviderWithEndpointsSummary>(`/api/admin/providers/${providerId}/summary`)
    return normalizeProviderSummary(response.data)
  })
}

/**
 * 更新 Provider 基础配置
 */
export async function updateProvider(
  providerId: string,
  data: Partial<{
    name: string
    description: string | null
    website: string
    provider_priority: number
    rpm_limit: number | null
    // 请求配置（从 Endpoint 迁移）
    max_retries: number
    cache_ttl_minutes: number  // 0表示不支持缓存，>0表示支持缓存并设置TTL(分钟)
    max_probe_interval_minutes: number
    is_active: boolean
    failover_rules: FailoverRulesConfig | null
    config: ProviderConfig | null
    responses_websocket_enabled: boolean
  }>,
  requestOptions?: ProviderRequestOptions,
): Promise<ProviderWithEndpointsSummary> {
  const response = await client.patch(`/api/admin/providers/${providerId}`, data, requestOptions)
  return normalizeProviderSummary(response.data)
}

/**
 * 创建 Provider
 */
export async function createProvider(
  data: {
    name: string
    description?: string
    website?: string
    provider_priority?: number
    is_active?: boolean
    max_retries?: number
    stream_first_byte_timeout?: number | null
    request_timeout?: number | null
    failover_rules?: FailoverRulesConfig | null
    config?: ProviderConfig | null
    responses_websocket_enabled?: boolean
  }
): Promise<{ id: string; name: string; message?: string }> {
  const response = await client.post('/api/admin/providers/', data)
  return response.data
}

/**
 * 删除 Provider
 */
export interface ProviderDeleteSubmitResponse {
  task_id: string
  status: string
  message: string
}

export interface ProviderDeleteTaskResponse {
  task_id: string
  provider_id: string
  status: string
  stage: string
  total_keys: number
  deleted_keys: number
  total_endpoints: number
  deleted_endpoints: number
  message: string
}

export async function deleteProvider(providerId: string): Promise<ProviderDeleteSubmitResponse> {
  const response = await client.delete<ProviderDeleteSubmitResponse>(`/api/admin/providers/${providerId}`)
  return response.data
}

export async function getProviderDeleteTask(
  providerId: string,
  taskId: string,
): Promise<ProviderDeleteTaskResponse> {
  const response = await client.get<ProviderDeleteTaskResponse>(
    `/api/admin/providers/${providerId}/delete-task/${taskId}`,
  )
  return response.data
}

/**
 * 映射预览相关类型
 */
export interface MappingMatchedModel {
  allowed_model: string
  mapping_pattern: string
}

export interface MappingMatchingGlobalModel {
  global_model_id: string
  global_model_name: string
  display_name: string
  is_active: boolean
  matched_models: MappingMatchedModel[]
}

export interface MappingMatchingKey {
  key_id: string
  key_name: string
  masked_key: string
  is_active: boolean
  allowed_models: string[]
  matching_global_models: MappingMatchingGlobalModel[]
}

export interface ProviderMappingPreviewResponse {
  provider_id: string
  provider_name: string
  keys: MappingMatchingKey[]
  total_keys: number
  total_matches: number
  // 截断提示
  truncated: boolean
  truncated_keys: number
  truncated_models: number
}

function mappingPreviewRecord(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {}
}

function mappingPreviewString(value: unknown, fallback = ''): string {
  return typeof value === 'string' ? value : fallback
}

function mappingPreviewCount(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0
    ? value
    : fallback
}

function normalizeProviderMappingPreview(
  value: unknown,
  providerId: string,
): ProviderMappingPreviewResponse {
  const source = mappingPreviewRecord(value)
  const rawKeys = Array.isArray(source.keys) ? source.keys : []
  const keys = rawKeys.map((rawKey) => {
    const key = mappingPreviewRecord(rawKey)
    const rawGlobalModels = Array.isArray(key.matching_global_models)
      ? key.matching_global_models
      : []

    return {
      key_id: mappingPreviewString(key.key_id),
      key_name: mappingPreviewString(key.key_name),
      masked_key: mappingPreviewString(key.masked_key, '***'),
      is_active: key.is_active === true,
      allowed_models: Array.isArray(key.allowed_models)
        ? key.allowed_models.filter((item): item is string => typeof item === 'string')
        : [],
      matching_global_models: rawGlobalModels.map((rawGlobalModel) => {
        const globalModel = mappingPreviewRecord(rawGlobalModel)
        const rawMatchedModels = Array.isArray(globalModel.matched_models)
          ? globalModel.matched_models
          : []

        return {
          global_model_id: mappingPreviewString(globalModel.global_model_id),
          global_model_name: mappingPreviewString(globalModel.global_model_name),
          display_name: mappingPreviewString(
            globalModel.display_name,
            mappingPreviewString(globalModel.global_model_name),
          ),
          is_active: globalModel.is_active === true,
          matched_models: rawMatchedModels.map((rawMatchedModel) => {
            const matchedModel = mappingPreviewRecord(rawMatchedModel)
            return {
              allowed_model: mappingPreviewString(matchedModel.allowed_model),
              mapping_pattern: mappingPreviewString(matchedModel.mapping_pattern),
            }
          }),
        }
      }),
    }
  })
  const inferredMatches = keys.reduce(
    (total, key) => total + key.matching_global_models.length,
    0,
  )

  return {
    provider_id: mappingPreviewString(source.provider_id, providerId),
    provider_name: mappingPreviewString(source.provider_name),
    keys,
    total_keys: mappingPreviewCount(source.total_keys, keys.length),
    total_matches: mappingPreviewCount(source.total_matches, inferredMatches),
    truncated: source.truncated === true,
    truncated_keys: mappingPreviewCount(source.truncated_keys, 0),
    truncated_models: mappingPreviewCount(source.truncated_models, 0),
  }
}

/**
 * 获取 Provider 映射预览
 */
export async function getProviderMappingPreview(
  providerId: string
): Promise<ProviderMappingPreviewResponse> {
  return dedupedRequest(`providers:mapping-preview:${providerId}`, async () => {
    const response = await client.get<ProviderMappingPreviewResponse>(`/api/admin/providers/${providerId}/mapping-preview`)
    return normalizeProviderMappingPreview(response.data, providerId)
  })
}
