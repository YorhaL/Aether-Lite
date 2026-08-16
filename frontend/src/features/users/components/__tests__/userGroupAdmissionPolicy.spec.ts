import { describe, expect, it } from 'vitest'
import type { UserGroup } from '@/api/users'
import type { UserGroupFormState } from '../user-management-types'
import {
  buildUserGroupAdmissionPayload,
  createUserGroupAdmissionForm,
  USER_GROUP_ADMISSION_POLICY_FIELDS,
} from '../userGroupAdmissionPolicy'

function formWithAdmission(
  patch: Partial<UserGroupFormState> = {},
): UserGroupFormState {
  return {
    name: 'Internal',
    allowed_providers_mode: 'unrestricted',
    allowed_api_formats_mode: 'unrestricted',
    allowed_models_mode: 'unrestricted',
    allowed_providers: [],
    allowed_api_formats: [],
    allowed_models: [],
    ...createUserGroupAdmissionForm(),
    ...patch,
  }
}

describe('user group admission policy form', () => {
  it('defines the same three policy categories as system admission settings', () => {
    expect(USER_GROUP_ADMISSION_POLICY_FIELDS.map((field) => field.valueKey)).toEqual([
      'rate_limit',
      'daily_usage_limit_usd',
      'concurrent_limit',
    ])
  })

  it('rehydrates custom concurrent limits and normalizes inherited modes to system', () => {
    const group = {
      rate_limit: 120,
      rate_limit_mode: 'custom',
      daily_usage_limit_usd: null,
      daily_usage_limit_mode: 'inherit',
      concurrent_limit: 8,
      concurrent_limit_mode: 'custom',
    } satisfies Pick<
      UserGroup,
      | 'rate_limit'
      | 'rate_limit_mode'
      | 'daily_usage_limit_usd'
      | 'daily_usage_limit_mode'
      | 'concurrent_limit'
      | 'concurrent_limit_mode'
    >

    expect(createUserGroupAdmissionForm(group)).toEqual({
      rate_limit_mode: 'custom',
      rate_limit: 120,
      daily_usage_limit_mode: 'system',
      daily_usage_limit_usd: undefined,
      concurrent_limit_mode: 'custom',
      concurrent_limit: 8,
    })
  })

  it('sends custom zero as unlimited and clears fields that follow the system', () => {
    const payload = buildUserGroupAdmissionPayload(formWithAdmission({
      rate_limit_mode: 'custom',
      rate_limit: undefined,
      daily_usage_limit_mode: 'system',
      daily_usage_limit_usd: 10,
      concurrent_limit_mode: 'custom',
      concurrent_limit: 0,
    }))

    expect(payload).toEqual({
      rate_limit_mode: 'custom',
      rate_limit: 0,
      daily_usage_limit_mode: 'system',
      daily_usage_limit_usd: null,
      concurrent_limit_mode: 'custom',
      concurrent_limit: 0,
    })
  })
})
