import { getI18nLocale } from '@/i18n'

export function walletStatusLabel(status: string | null | undefined): string {
  const labels: Record<string, string> = {
    active: '正常',
    suspended: '已冻结',
    closed: '已关闭',
  }
  if (!status) return '未知'
  if (getI18nLocale() === 'en-US') {
    const englishLabels: Record<string, string> = {
      active: 'Active',
      suspended: 'Frozen',
      closed: 'Closed',
    }
    return englishLabels[status] || status
  }
  return labels[status] || status
}

export function formatWalletCurrency(
  value: number | null | undefined,
  options?: { decimals?: number }
): string {
  const decimals = options?.decimals ?? 2
  const amount = Number(value ?? 0)
  return `$${amount.toFixed(decimals)}`
}

export function walletStatusBadge(status: string | null | undefined): string {
  if (status === 'active') return 'success'
  if (status === 'suspended') return 'warning'
  if (status === 'closed') return 'destructive'
  return 'secondary'
}
