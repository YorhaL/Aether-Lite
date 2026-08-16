import apiClient from './client'

export interface CandidateRankingMetadata {
  mode?: string
  priority_mode?: string
  index?: number
  priority_slot?: number
  promoted_by?: string
}

export interface ImageProgress {
  phase?: 'upstream_connecting' | 'upstream_streaming' | 'upstream_completed' | 'failed' | string
  upstream_ttfb_ms?: number | null
  upstream_sse_frame_count?: number | null
  last_upstream_event?: string | null
  last_upstream_frame_at_unix_ms?: number | null
  partial_image_count?: number | null
  last_client_visible_event?: string | null
  downstream_heartbeat_count?: number | null
  last_downstream_heartbeat_at_unix_ms?: number | null
  downstream_heartbeat_interval_ms?: number | null
}

export interface CandidateResponseBoundary {
  source?: string
  status_code?: number | null
  headers?: Record<string, unknown> | null
  body?: unknown
  body_ref?: string | null
  body_state?: string | null
}

export interface CandidateRecord {
  id: string
  request_id: string
  candidate_index: number
  retry_index: number
  provider_id?: string
  provider_name?: string
  provider_website?: string  // Provider 官网
  provider_priority?: number
  endpoint_id?: string
  endpoint_name?: string  // 端点显示名称（api_format）
  endpoint_api_family?: string
  endpoint_kind?: string
  key_id?: string
  key_name?: string  // 密钥名称
  key_account_label?: string
  key_preview?: string  // 密钥脱敏预览（如 sk-***abc）
  key_auth_type?: string  // 密钥认证类型（api_key, service_account, bearer）
  key_api_formats?: unknown
  key_internal_priority?: number
  key_global_priority_by_format?: Record<string, number> | null
  key_capabilities?: Record<string, boolean> | null  // Key 支持的能力
  required_capabilities?: Record<string, boolean> | null  // 请求实际需要的能力标签
  status: 'pending' | 'streaming' | 'success' | 'failed' | 'skipped' | 'cancelled' | 'available' | 'unused' | 'stream_interrupted'
  skip_reason?: string
  is_cached: boolean
  // 执行结果字段
  status_code?: number
  error_type?: string
  error_message?: string
  latency_ms?: number
  concurrent_requests?: number
  ranking?: CandidateRankingMetadata | null
  image_progress?: ImageProgress | null
  extra_data?: Record<string, unknown> & {
    upstream_response?: CandidateResponseBoundary
    image_progress?: ImageProgress | null
  }
  created_at: string
  started_at?: string
  finished_at?: string
}

export interface RequestTrace {
  request_id: string
  request_path?: string
  request_query_string?: string
  request_path_and_query?: string
  total_candidates: number
  final_status: 'success' | 'failed' | 'streaming' | 'pending' | 'cancelled'
  total_latency_ms: number
  candidates: CandidateRecord[]
}

export interface ProviderStats {
  total_attempts: number
  success_count: number
  failed_count: number
  cancelled_count: number
  skipped_count: number
  pending_count: number
  available_count: number
  failure_rate: number
}

export const requestTraceApi = {
  /**
   * 获取特定请求的完整追踪信息
   */
  async getRequestTrace(
    requestId: string,
    options: { attemptedOnly?: boolean } = {},
  ): Promise<RequestTrace> {
    const attemptedOnly = options.attemptedOnly ?? false
    const response = await apiClient.get<RequestTrace>(`/api/admin/monitoring/trace/${requestId}`, {
      params: { attempted_only: attemptedOnly },
    })
    return response.data
  },

  /**
   * 获取某个 Provider 的失败率统计
   */
  async getProviderStats(providerId: string, limit: number = 100): Promise<ProviderStats> {
    const response = await apiClient.get<ProviderStats>(`/api/admin/monitoring/trace/stats/provider/${providerId}`, {
      params: { limit }
    })
    return response.data
  }
}
