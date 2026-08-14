import type { User } from '@/api/users'

type UserRateLimitPolicyInput = Pick<User, 'rate_limit' | 'effective_policy'>
type Translate = (key: string) => string

export function resolveUserEffectiveRateLimit(
  user: UserRateLimitPolicyInput,
): number | null | undefined {
  const policy = user.effective_policy?.rate_limit
  return policy ? policy.value : user.rate_limit
}

export function formatUserEffectiveRateLimitSource(
  user: UserRateLimitPolicyInput,
  translate: Translate,
  locale: string,
): string {
  const policy = user.effective_policy?.rate_limit
  if (!policy) return ''
  if (policy.source === 'group' && policy.group_name) {
    return `${translate('继承自分组：')}${policy.group_name}`
  }
  if (policy.source === 'combined') {
    const groupNames = Array.isArray(policy.group_names)
      ? policy.group_names.join(locale === 'en-US' ? ', ' : '、')
      : ''
    return groupNames
      ? `${translate('继承自多个分组：')}${groupNames}`
      : translate('继承自多个分组')
  }
  if (policy.source === 'user') {
    return translate('用户单独配置')
  }
  return translate('系统默认')
}
