export interface SystemAdmissionPolicyConfig {
  rate_limit_per_minute: number
  daily_usage_limit_usd: number
  concurrent_limit: number
}

export type SystemAdmissionPolicyConfigKey = keyof SystemAdmissionPolicyConfig

export interface SystemAdmissionPolicyField {
  key: SystemAdmissionPolicyConfigKey
  label: string
  unit: string
  description: string
  step: number
  integer: boolean
  max?: number
}

export const SYSTEM_ADMISSION_POLICY_FIELDS: readonly SystemAdmissionPolicyField[] = [
  {
    key: 'rate_limit_per_minute',
    label: 'RPM 限制',
    unit: '请求/分钟',
    description: '系统每分钟请求限制（0 表示不限制）',
    step: 1,
    integer: true,
    max: 4_294_967_295,
  },
  {
    key: 'daily_usage_limit_usd',
    label: '每日额度限制',
    unit: '美元/日',
    description: '系统每日额度限制（美元/日，0 表示不限制）',
    step: 0.01,
    integer: false,
  },
  {
    key: 'concurrent_limit',
    label: '并发限制',
    unit: '请求',
    description: '系统并发请求限制（0 表示不限制）',
    step: 1,
    integer: true,
    max: 4_294_967_295,
  },
]

export function createDefaultSystemAdmissionPolicyConfig(): SystemAdmissionPolicyConfig {
  return {
    rate_limit_per_minute: 0,
    daily_usage_limit_usd: 0,
    concurrent_limit: 0,
  }
}

export function normalizeSystemAdmissionPolicyValue(
  field: SystemAdmissionPolicyField,
  rawValue: unknown,
): number {
  const parsed = Number(rawValue)
  if (!Number.isFinite(parsed)) return 0

  let normalized = Math.max(0, parsed)
  if (field.integer) normalized = Math.trunc(normalized)
  if (field.max !== undefined) normalized = Math.min(field.max, normalized)
  return normalized
}
