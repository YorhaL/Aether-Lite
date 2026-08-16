import apiClient from './client'

export interface WalletSummary {
  id: string
  balance: number
  currency: string
  status: string
  limit_mode?: 'finite' | 'unlimited'
  unlimited?: boolean
  total_consumed: number
  updated_at: string
}

export interface WalletBalanceResponse {
  wallet: WalletSummary | null
  unlimited: boolean
  limit_mode: 'finite' | 'unlimited'
  balance: number | null
  currency: string
}

export const walletApi = {
  async getBalance(): Promise<WalletBalanceResponse> {
    const response = await apiClient.get<WalletBalanceResponse>('/api/wallet/balance')
    return response.data
  },
}
