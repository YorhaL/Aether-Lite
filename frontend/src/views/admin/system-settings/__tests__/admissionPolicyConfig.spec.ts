import { describe, expect, it } from 'vitest'
import {
  normalizeSystemAdmissionPolicyValue,
  SYSTEM_ADMISSION_POLICY_FIELDS,
} from '../admissionPolicyConfig'

describe('system admission policy config', () => {
  it('normalizes all fields with the shared non-negative input rules', () => {
    const rpm = SYSTEM_ADMISSION_POLICY_FIELDS.find(
      (field) => field.key === 'rate_limit_per_minute',
    )!
    const daily = SYSTEM_ADMISSION_POLICY_FIELDS.find(
      (field) => field.key === 'daily_usage_limit_usd',
    )!
    const concurrent = SYSTEM_ADMISSION_POLICY_FIELDS.find(
      (field) => field.key === 'concurrent_limit',
    )!

    expect(normalizeSystemAdmissionPolicyValue(rpm, 12.8)).toBe(12)
    expect(normalizeSystemAdmissionPolicyValue(daily, 12.8)).toBe(12.8)
    expect(normalizeSystemAdmissionPolicyValue(concurrent, -1)).toBe(0)
    expect(normalizeSystemAdmissionPolicyValue(concurrent, 'invalid')).toBe(0)
  })
})
