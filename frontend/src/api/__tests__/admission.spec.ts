import { beforeEach, describe, expect, it, vi } from 'vitest'

const { getMock } = vi.hoisted(() => ({
  getMock: vi.fn(),
}))

vi.mock('@/api/client', () => ({
  default: {
    get: getMock,
  },
}))

import { admissionApi } from '@/api/admission'

describe('admissionApi', () => {
  beforeEach(() => {
    getMock.mockReset()
    getMock.mockResolvedValue({ data: { user_id: 'user-1', rules: [] } })
  })

  it('loads the authenticated account admission status', async () => {
    await admissionApi.getAccountStatus()

    expect(getMock).toHaveBeenCalledWith('/api/monitoring/rate-limit-status')
  })
})
