import type { RateLimitPolicyMode, UpsertUserGroupRequest, UserGroup } from '@/api/users'
import type { UserGroupFormState } from './user-management-types'

export type UserGroupAdmissionValueKey =
  | 'rate_limit'
  | 'daily_usage_limit_usd'
  | 'concurrent_limit'

export type UserGroupAdmissionModeKey =
  | 'rate_limit_mode'
  | 'daily_usage_limit_mode'
  | 'concurrent_limit_mode'

export interface UserGroupAdmissionPolicyField {
  valueKey: UserGroupAdmissionValueKey
  modeKey: UserGroupAdmissionModeKey
  label: string
  unit: string
  step: number
  integer: boolean
  max?: number
}

export const USER_GROUP_ADMISSION_POLICY_FIELDS: readonly UserGroupAdmissionPolicyField[] = [
  {
    valueKey: 'rate_limit',
    modeKey: 'rate_limit_mode',
    label: 'RPM 限制',
    unit: '请求/分钟',
    step: 1,
    integer: true,
    max: 2_147_483_647,
  },
  {
    valueKey: 'daily_usage_limit_usd',
    modeKey: 'daily_usage_limit_mode',
    label: '每日额度限制',
    unit: '美元/日',
    step: 0.01,
    integer: false,
  },
  {
    valueKey: 'concurrent_limit',
    modeKey: 'concurrent_limit_mode',
    label: '并发限制',
    unit: '请求',
    step: 1,
    integer: true,
    max: 4_294_967_295,
  },
]

export function normalizeUserGroupAdmissionMode(
  mode: RateLimitPolicyMode | undefined,
): RateLimitPolicyMode {
  return mode === 'custom' ? 'custom' : 'system'
}

export function createUserGroupAdmissionForm(
  group?: Pick<
    UserGroup,
    | 'rate_limit'
    | 'rate_limit_mode'
    | 'daily_usage_limit_usd'
    | 'daily_usage_limit_mode'
    | 'concurrent_limit'
    | 'concurrent_limit_mode'
  >,
): Pick<
  UserGroupFormState,
  | UserGroupAdmissionValueKey
  | UserGroupAdmissionModeKey
> {
  return {
    rate_limit_mode: normalizeUserGroupAdmissionMode(group?.rate_limit_mode),
    rate_limit: group?.rate_limit ?? undefined,
    daily_usage_limit_mode: normalizeUserGroupAdmissionMode(group?.daily_usage_limit_mode),
    daily_usage_limit_usd: group?.daily_usage_limit_usd ?? undefined,
    concurrent_limit_mode: normalizeUserGroupAdmissionMode(group?.concurrent_limit_mode),
    concurrent_limit: group?.concurrent_limit ?? undefined,
  }
}

export function buildUserGroupAdmissionPayload(
  form: UserGroupFormState,
): Pick<
  UpsertUserGroupRequest,
  | UserGroupAdmissionValueKey
  | UserGroupAdmissionModeKey
> {
  return {
    rate_limit_mode: form.rate_limit_mode,
    rate_limit: form.rate_limit_mode === 'custom' ? (form.rate_limit ?? 0) : null,
    daily_usage_limit_mode: form.daily_usage_limit_mode,
    daily_usage_limit_usd: form.daily_usage_limit_mode === 'custom'
      ? (form.daily_usage_limit_usd ?? 0)
      : null,
    concurrent_limit_mode: form.concurrent_limit_mode,
    concurrent_limit: form.concurrent_limit_mode === 'custom'
      ? (form.concurrent_limit ?? 0)
      : null,
  }
}
