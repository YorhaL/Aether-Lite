import apiClient from './client'

export type AccountAdmissionRuleKind =
  | 'request_count'
  | 'concurrent_requests'
  | 'usage_cost_usd'

export type AccountAdmissionRuleStatus = 'available' | 'unlimited' | 'unavailable'

export interface AccountAdmissionRule {
  kind: AccountAdmissionRuleKind
  available: boolean
  status: AccountAdmissionRuleStatus
  limit: number
  used: number | null
  remaining: number | null
  window_seconds?: number
  period?: 'calendar_day'
  timezone?: string
  window_start?: string
  window_end?: string
  reset_at?: string
}

export interface AccountAdmissionStatusResponse {
  user_id: string
  rules: AccountAdmissionRule[]
}

export const admissionApi = {
  async getAccountStatus(): Promise<AccountAdmissionStatusResponse> {
    const response = await apiClient.get<AccountAdmissionStatusResponse>(
      '/api/monitoring/rate-limit-status'
    )
    return response.data
  },
}
