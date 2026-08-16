/**
 * 请求头规则类型
 * - set: 设置/覆盖请求头
 * - drop: 删除请求头
 * - rename: 重命名请求头（保留原值）
 */
export interface HeaderRuleSet {
  action: 'set'
  key: string
  value: string
}

export interface HeaderRuleDrop {
  action: 'drop'
  key: string
}

export interface HeaderRuleRename {
  action: 'rename'
  from: string
  to: string
}

/**
 * 请求体规则类型
 * - set: 设置/覆盖字段
 * - drop: 删除字段
 * - rename: 重命名字段（保留原值）
 */
/**
 * 请求体规则 - 覆写字段
 *
 * - path 支持嵌套路径，如 "metadata.user.name"
 * - 使用 "\." 转义字面量点号，如 "config\.v1.enabled"
 */
export interface BodyRuleSet {
  action: 'set'
  path: string
  value: unknown
}

/**
 * 请求体规则 - 删除字段
 *
 * - path 支持嵌套路径，如 "metadata.internal_flag"
 * - 使用 "\." 转义字面量点号，如 "config\.v1.enabled"
 */
export interface BodyRuleDrop {
  action: 'drop'
  path: string
}

/**
 * 请求体规则 - 重命名/移动字段
 *
 * - from/to 支持嵌套路径，如 "extra.old_config" -> "settings.new_config"
 * - 使用 "\." 转义字面量点号，如 "config\.v1.enabled"
 */
export interface BodyRuleRename {
  action: 'rename'
  from: string
  to: string
}

/**
 * 请求体规则 - 向数组追加元素
 *
 * - path 指向目标数组，如 "messages"
 * - value 为要追加的元素
 */
export interface BodyRuleAppend {
  action: 'append'
  path: string
  value: unknown
}

/**
 * 请求体规则 - 在数组指定位置插入元素
 *
 * - path 指向目标数组，如 "messages"
 * - index 为插入位置（支持负数）
 * - value 为要插入的元素
 */
export interface BodyRuleInsert {
  action: 'insert'
  path: string
  index: number
  value: unknown
}

/**
 * 请求体规则 - 正则替换字符串值
 *
 * - path 指向目标字符串字段，如 "messages[0].content"
 * - pattern 为正则表达式
 * - replacement 为替换字符串
 * - flags 可选，支持 i(忽略大小写)/m(多行)/s(dotall)
 * - count 替换次数，0=全部替换（默认）
 */
export interface BodyRuleRegexReplace {
  action: 'regex_replace'
  path: string
  pattern: string
  replacement: string
  flags?: string
  count?: number
}

export type BodyRuleConditionOp =
  | 'eq' | 'neq'
  | 'gt' | 'lt' | 'gte' | 'lte'
  | 'starts_with' | 'ends_with' | 'contains' | 'matches'
  | 'exists' | 'not_exists'
  | 'in' | 'type_is'

export interface BodyRuleConditionLeaf {
  path: string
  op: BodyRuleConditionOp
  value?: unknown  // exists / not_exists 不需要 value
  source?: 'body' | 'current' | 'original' | 'request_headers' | 'headers'
}

export interface BodyRuleConditionAll {
  all: BodyRuleCondition[]
}

export interface BodyRuleConditionAny {
  any: BodyRuleCondition[]
}

export type BodyRuleCondition =
  | BodyRuleConditionLeaf
  | BodyRuleConditionAll
  | BodyRuleConditionAny

export type HeaderRule = (HeaderRuleSet | HeaderRuleDrop | HeaderRuleRename) & {
  condition?: BodyRuleCondition
  enabled?: boolean
}

export type BodyRule = (BodyRuleSet | BodyRuleDrop | BodyRuleRename | BodyRuleAppend | BodyRuleInsert | BodyRuleRegexReplace) & {
  condition?: BodyRuleCondition
  enabled?: boolean
}

export interface ChatPiiRedactionProviderConfig {
  enabled: boolean
}

export interface ProviderConfig {
  chat_pii_redaction?: ChatPiiRedactionProviderConfig
  failover_rules?: FailoverRulesConfig
  [key: string]: unknown
}

export interface ProviderEndpoint {
  id: string
  provider_id: string
  provider_name: string
  api_format: string
  base_url: string
  custom_path?: string  // 自定义请求路径（可选，为空则使用 API 格式默认路径）
  // 请求头配置
  header_rules?: HeaderRule[]  // 请求头规则列表，支持 set/drop/rename 操作
  // 请求体配置
  body_rules?: BodyRule[]  // 请求体规则列表，支持 set/drop/rename 操作
  max_retries: number
  is_active: boolean
  config?: Record<string, unknown>
  total_keys: number
  active_keys: number
  created_at: string
  updated_at: string
}

/**
 * 模型权限配置类型
 *
 * 使用示例：
 * 1. 不限制（允许所有模型）: null
 * 2. 白名单模式: ["gpt-4", "claude-3-opus"]
 */
export type AllowedModels = string[] | null

// AllowedModels 类型守卫函数
export function isAllowedModelsList(value: AllowedModels): value is string[] {
  return Array.isArray(value)
}

export interface EndpointAPIKey {
  id: string
  provider_id: string
  api_formats: string[]  // 支持的 endpoint signature 列表（如 "openai:chat"）
  api_key_masked: string
  api_key_plain?: string | null
  auth_type: 'api_key' | 'bearer'  // 认证类型（必返回）
  name: string  // 密钥名称（必填，用于识别）
  rate_multipliers?: Record<string, number> | null  // 按 endpoint signature 的成本倍率
  internal_priority: number  // Key 内部优先级
  global_priority_by_format?: Record<string, number> | null  // 按 endpoint signature 的全局优先级
  rpm_limit?: number | null  // RPM 速率限制 (1-10000)，null 表示自适应模式
  concurrent_limit?: number | null  // 并发请求上限，null/0 表示不限制
  allowed_models?: AllowedModels  // 允许使用的模型列表（null=不限制）
  capabilities?: Record<string, boolean> | null  // 能力标签配置（如 cache_1h, context_1m）
  // 缓存与熔断配置
  cache_ttl_minutes: number  // 缓存 TTL（分钟），0=禁用
  max_probe_interval_minutes: number  // 熔断探测间隔（分钟）
  // 按 endpoint signature 的健康度数据
  health_by_format?: Record<string, FormatHealthData>
  circuit_breaker_by_format?: Record<string, FormatCircuitBreakerData>
  // 聚合字段（从 health_by_format 计算，用于列表显示）
  health_score: number
  circuit_breaker_open?: boolean
  consecutive_failures: number
  last_failure_at?: string
  request_count: number
  success_count: number
  error_count: number
  success_rate: number
  avg_response_time_ms: number
  is_active: boolean
  note?: string  // 备注说明（可选）
  last_used_at?: string
  created_at: string
  updated_at: string
  // 自适应 RPM 字段
  is_adaptive?: boolean  // 是否为自适应模式（rpm_limit=NULL）
  effective_limit?: number | null  // 当前有效 RPM 限制（自适应使用学习值，固定使用配置值，未学习时为 null）
  learned_rpm_limit?: number | null  // 学习到的 RPM 限制
  // 滑动窗口利用率采样
  utilization_samples?: Array<{ ts: number; util: number }>  // 利用率采样窗口
  last_probe_increase_at?: string  // 上次探测性扩容时间
  concurrent_429_count?: number
  rpm_429_count?: number
  last_429_at?: string
  last_429_type?: string
  // 单格式场景的熔断器字段
  circuit_breaker_open_at?: string
  next_probe_at?: string
  half_open_until?: string
  half_open_successes?: number
  half_open_failures?: number
  request_results_window?: Array<{ ts: number; ok: boolean }>  // 请求结果滑动窗口
  // 自动获取模型
  auto_fetch_models?: boolean  // 是否启用自动获取模型
  last_models_fetch_at?: string  // 最后获取模型时间
  last_models_fetch_error?: string  // 最后获取模型错误信息
  locked_models?: string[]  // 被锁定的模型列表
  // 模型过滤规则（仅当 auto_fetch_models=true 时生效）
  model_include_patterns?: string[]  // 模型包含规则（支持 * 和 ? 通配符）
  model_exclude_patterns?: string[]  // 模型排除规则（支持 * 和 ? 通配符）
}

// 按格式的健康度数据
export interface FormatHealthData {
  health_score: number
  error_rate: number
  window_size: number
  consecutive_failures: number
  last_failure_at?: string | null
  circuit_breaker: FormatCircuitBreakerData
}

// 按格式的熔断器数据
export interface FormatCircuitBreakerData {
  open: boolean
  reason?: string | null
  open_at?: string | null
  next_probe_at?: string | null
  next_probe_at_unix_secs?: number | null
  probe_interval_minutes?: number | null
  max_probe_interval_minutes?: number | null
  failure_count?: number | null
  consecutive_failures?: number | null
  last_failure_at?: string | null
  last_probe_failure_at?: string | null
  half_open_until?: string | null
  half_open_successes: number
  half_open_failures: number
  request_results_window?: Array<{ ts: number; ok: boolean }>
}

export interface EndpointAPIKeyUpdate {
  api_formats?: string[]  // 支持的 API 格式列表
  name?: string
  api_key?: string  // 仅在需要更新时提供
  auth_type?: 'api_key' | 'bearer'  // 认证类型
  rate_multipliers?: Record<string, number> | null  // 按 API 格式的成本倍率
  internal_priority?: number
  global_priority_by_format?: Record<string, number> | null  // 按 API 格式的全局优先级
  rpm_limit?: number | null  // RPM 速率限制 (1-10000)，null 表示切换为自适应模式
  concurrent_limit?: number | null  // 并发请求上限，null/0 表示不限制
  allowed_models?: AllowedModels
  capabilities?: Record<string, boolean> | null
  cache_ttl_minutes?: number
  max_probe_interval_minutes?: number
  note?: string
  is_active?: boolean
  auto_fetch_models?: boolean  // 是否启用自动获取模型
  locked_models?: string[]  // 被锁定的模型列表
  // 模型过滤规则（仅当 auto_fetch_models=true 时生效）
  model_include_patterns?: string[]  // 模型包含规则（支持 * 和 ? 通配符）
  model_exclude_patterns?: string[]  // 模型排除规则（支持 * 和 ? 通配符）
}

export interface EndpointHealthDetail {
  api_format: string
  health_score: number
  is_active: boolean
  total_keys?: number
  active_keys?: number
}

export interface EndpointHealthEvent {
  timestamp: string
  status: 'success' | 'failed' | 'skipped' | 'started'
  status_code?: number | null
  latency_ms?: number | null
  error_type?: string | null
  error_message?: string | null
}

export interface HealthTimelineDetail {
  segment_index?: number
  status?: string
  time_range_start?: string | null
  time_range_end?: string | null
  total_attempts?: number | null
  success_count?: number | null
  failed_count?: number | null
  success_rate?: number | null
  avg_latency_ms?: number | null
  avg_first_byte_ms?: number | null
  avg_tps?: number | null
}

export interface EndpointStatusMonitor {
  api_format: string
  total_attempts: number
  success_count: number
  failed_count: number
  skipped_count: number
  success_rate: number
  avg_latency_ms?: number | null
  avg_first_byte_ms?: number | null
  avg_tps?: number | null
  provider_count: number
  key_count: number
  last_event_at?: string | null
  events: EndpointHealthEvent[]
  timeline?: string[]
  timeline_details?: HealthTimelineDetail[]
  time_range_start?: string | null
  time_range_end?: string | null
}

export interface EndpointStatusMonitorResponse {
  generated_at: string
  formats: EndpointStatusMonitor[]
}

// 公开版事件（不含敏感信息如 provider_id, key_id）
export interface PublicHealthEvent {
  timestamp: string
  status: string
  status_code?: number | null
  latency_ms?: number | null
  error_type?: string | null
}

// 公开版端点状态监控类型（返回 events，前端复用 EndpointHealthTimeline 组件）
export interface PublicEndpointStatusMonitor {
  api_format: string
  api_path: string  // 本站入口路径
  total_attempts: number
  success_count: number
  failed_count: number
  skipped_count: number
  success_rate: number
  avg_latency_ms?: number | null
  avg_first_byte_ms?: number | null
  avg_tps?: number | null
  last_event_at?: string | null
  events: PublicHealthEvent[]
  timeline?: string[]
  timeline_details?: HealthTimelineDetail[]
  time_range_start?: string | null
  time_range_end?: string | null
}

export interface PublicEndpointStatusMonitorResponse {
  generated_at: string
  formats: PublicEndpointStatusMonitor[]
}

export interface ModelHealthEvent {
  timestamp: string
  status: 'success' | 'failed'
  status_code?: number | null
  latency_ms?: number | null
  first_byte_time_ms?: number | null
  error_type?: string | null
}

export interface ModelStatusMonitor {
  model: string
  display_name?: string | null
  total_attempts: number
  success_count: number
  failed_count: number
  success_rate: number
  avg_latency_ms?: number | null
  avg_first_byte_ms?: number | null
  avg_tps?: number | null
  provider_count?: number
  last_event_at?: string | null
  events: ModelHealthEvent[]
  timeline?: string[]
  timeline_details?: HealthTimelineDetail[]
  time_range_start?: string | null
  time_range_end?: string | null
}

export interface ModelStatusMonitorResponse {
  generated_at: string
  models: ModelStatusMonitor[]
}

export interface ProviderStatusMonitor {
  provider_id: string
  provider_name: string
  is_active: boolean
  total_attempts: number
  success_count: number
  failed_count: number
  success_rate: number
  avg_latency_ms?: number | null
  avg_first_byte_ms?: number | null
  avg_tps?: number | null
  model_count: number
  last_event_at?: string | null
  timeline?: string[]
  timeline_details?: HealthTimelineDetail[]
  time_range_start?: string | null
  time_range_end?: string | null
  models: ModelStatusMonitor[]
}

export interface ProviderStatusMonitorResponse {
  generated_at: string
  providers: ProviderStatusMonitor[]
}

export type HealthMonitorRelatedDimension = 'endpoint' | 'model' | 'provider'

export interface HealthRelatedMonitor {
  kind: HealthMonitorRelatedDimension
  key: string
  display_name: string
  meta_text?: string | null
  total_attempts: number
  success_count: number
  failed_count: number
  success_rate: number
  avg_latency_ms?: number | null
  avg_first_byte_ms?: number | null
  avg_tps?: number | null
  last_event_at?: string | null
  timeline?: string[]
  timeline_details?: HealthTimelineDetail[]
  time_range_start?: string | null
  time_range_end?: string | null
}

export interface HealthRelatedMonitorResponse {
  generated_at: string
  dimension: HealthMonitorRelatedDimension
  value: string
  related_endpoints: HealthRelatedMonitor[]
  related_models: HealthRelatedMonitor[]
  related_providers: HealthRelatedMonitor[]
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

export function normalizeChatPiiRedactionProviderConfig(value: unknown): ChatPiiRedactionProviderConfig {
  if (!isPlainObject(value) || typeof value.enabled !== 'boolean') {
    return { enabled: false }
  }
  return { enabled: value.enabled }
}

export interface FailoverRuleItem {
  pattern: string
  description?: string
  status_codes?: number[]
}

export interface FailoverRulesConfig {
  max_retries?: number
  stop_on_transport_errors?: boolean
  stop_status_codes?: number[]
  stop_on_status_codes?: number[]
  early_stop_status_codes?: number[]
  non_retryable_status_codes?: number[]
  continue_on_status_codes?: number[]
  retryable_status_codes?: number[]
  retry_on_status_codes?: number[]
  continue_status_codes?: number[]
  success_failover_patterns?: FailoverRuleItem[]
  error_stop_patterns?: FailoverRuleItem[]
}

export interface ProviderWithEndpointsSummary {
  id: string
  name: string
  description?: string
  website?: string
  provider_priority: number
  // 请求配置（从 Endpoint 迁移）
  max_retries?: number  // 最大重试次数
  // 超时配置（秒），为空时使用全局配置
  stream_first_byte_timeout?: number  // 流式请求首字节超时
  request_timeout?: number  // 非流式请求整体超时
  is_active: boolean
  total_endpoints: number
  active_endpoints: number
  total_keys: number
  active_keys: number
  total_models: number
  active_models: number
  global_model_ids: string[]
  avg_health_score: number
  unhealthy_endpoints: number
  api_formats: string[]
  endpoint_health_details: EndpointHealthDetail[]
  chat_pii_redaction?: ChatPiiRedactionProviderConfig | null
  failover_rules?: FailoverRulesConfig | null
  created_at: string
  updated_at: string
}

export interface HealthStatus {
  endpoint_id?: string
  endpoint_health_score?: number
  endpoint_consecutive_failures?: number
  endpoint_last_failure_at?: string
  endpoint_is_active?: boolean
  key_id?: string
  key_health_score?: number
  key_consecutive_failures?: number
  key_last_failure_at?: string
  key_is_active?: boolean
  key_statistics?: Record<string, unknown>
}

export interface HealthSummary {
  endpoints: {
    total: number
    active: number
    unhealthy: number
  }
  keys: {
    total: number
    active: number
    unhealthy: number
  }
}

export interface KeyRpmStatus {
  key_id: string
  current_rpm: number
  rpm_limit?: number
}

export interface ProviderModelMapping {
  name: string
  priority: number  // 优先级（数字越小优先级越高）
  api_formats?: string[]  // 作用域（适用的 API 格式），为空表示对所有格式生效
  endpoint_ids?: string[]  // 作用域（适用的端点 ID），为空表示对所有端点生效
  operations?: string[]  // 作用域（适用的请求操作），为空表示对该格式的全部操作生效
}

// 保留别名以保持向后兼容
export type ProviderModelAlias = ProviderModelMapping

export interface AdaptiveStatsResponse {
  adaptive_mode: boolean
  current_limit: number | null
  learned_limit: number | null
  concurrent_429_count: number
  rpm_429_count: number
  last_429_at: string | null
  last_429_type: string | null
  adjustment_count: number
  recent_adjustments: Array<{
    timestamp: string
    old_limit: number
    new_limit: number
    reason: string
    [key: string]: unknown
  }>
}
