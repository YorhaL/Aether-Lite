import apiClient from './client'
import { buildCacheKey, cachedRequest } from '@/utils/cache'
import type { WalletSummary } from './wallet'

export interface AdminWallet extends WalletSummary {
  user_id: string | null
  api_key_id: string | null
  owner_type: 'user' | 'api_key'
  owner_name: string | null
  created_at: string
}

export interface AdminWalletListResponse {
  items: AdminWallet[]
  total: number
  limit: number
  offset: number
}

export type AdminWalletDetailResponse = AdminWallet

export interface WalletAdjustRequest {
  amount_usd: number
  description?: string
}

export const adminWalletApi = {
  async listWallets(params?: {
    status?: string
    owner_type?: 'user' | 'api_key'
    limit?: number
    offset?: number
  }): Promise<AdminWalletListResponse> {
    const response = await apiClient.get<AdminWalletListResponse>('/api/admin/wallets', { params })
    return response.data
  },

  async listAllWallets(params?: {
    status?: string
    owner_type?: 'user' | 'api_key'
  }, options: { cacheTtlMs?: number } = {}): Promise<AdminWallet[]> {
    const cacheKey = buildCacheKey('admin:wallets:list-all', params as Record<string, unknown> | undefined)
    return cachedRequest(cacheKey, async () => {
      const items: AdminWallet[] = []
      const limit = 200
      let offset = 0

      for (let page = 0; page < 200; page += 1) {
        const data = await this.listWallets({ ...params, limit, offset })
        items.push(...data.items)
        if (items.length >= data.total || data.items.length < limit) return items
        offset += data.items.length
      }

      throw new Error('钱包列表分页超过安全上限，已中止请求')
    }, options.cacheTtlMs ?? 0)
  },

  async getWalletDetail(walletId: string): Promise<AdminWalletDetailResponse> {
    const response = await apiClient.get<AdminWalletDetailResponse>(`/api/admin/wallets/${walletId}`)
    return response.data
  },

  async adjustWallet(walletId: string, payload: WalletAdjustRequest): Promise<{ wallet: AdminWallet }> {
    const response = await apiClient.post<{ wallet: AdminWallet }>(`/api/admin/wallets/${walletId}/adjust`, {
      ...payload,
    })
    return response.data
  },
}
