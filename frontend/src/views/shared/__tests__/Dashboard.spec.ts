import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, defineComponent, h, nextTick, type App } from 'vue'

import Dashboard from '../Dashboard.vue'

const dashboardApiMocks = vi.hoisted(() => ({
  getStats: vi.fn(),
  getDailyStats: vi.fn(),
}))

const admissionApiMocks = vi.hoisted(() => ({
  getAccountStatus: vi.fn(),
}))

const authStoreMock = vi.hoisted(() => ({
  canAccessAdmin: false,
  isAdmin: false,
  isAuditAdmin: false,
}))

vi.mock('@/stores/auth', () => ({
  useAuthStore: () => authStoreMock,
}))

vi.mock('@/api/dashboard', () => ({
  dashboardApi: dashboardApiMocks,
}))

vi.mock('@/api/admission', () => ({
  admissionApi: admissionApiMocks,
}))

vi.mock('@/api/announcements', () => ({
  announcementApi: {
    getAnnouncements: vi.fn().mockResolvedValue({ items: [] }),
    markAsRead: vi.fn().mockResolvedValue({}),
  },
}))

vi.mock('@/components/charts/BarChart.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return { default: defineComponent({ name: 'BarChartStub', setup: () => () => h('div') }) }
})

vi.mock('@/components/charts/DoughnutChart.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return { default: defineComponent({ name: 'DoughnutChartStub', setup: () => () => h('div') }) }
})

vi.mock('@/components/charts/LineChart.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return { default: defineComponent({ name: 'LineChartStub', setup: () => () => h('div') }) }
})

vi.mock('@/components/common', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    TimeRangePicker: defineComponent({
      name: 'TimeRangePickerStub',
      setup() {
        return () => h('div')
      },
    }),
  }
})

vi.mock('@/components/ui', async () => {
  const { defineComponent, h } = await import('vue')
  const passthrough = (name: string, tag = 'div') => defineComponent({
    name,
    setup(_, { slots }) {
      return () => h(tag, slots.default?.())
    },
  })
  return {
    Card: passthrough('CardStub', 'section'),
    Badge: passthrough('BadgeStub', 'span'),
    Button: passthrough('ButtonStub', 'button'),
    Skeleton: defineComponent({ name: 'SkeletonStub', setup: () => () => h('div') }),
    Dialog: passthrough('DialogStub'),
    Table: passthrough('TableStub', 'table'),
    TableHeader: passthrough('TableHeaderStub', 'thead'),
    TableBody: passthrough('TableBodyStub', 'tbody'),
    TableRow: passthrough('TableRowStub', 'tr'),
    TableHead: passthrough('TableHeadStub', 'th'),
    TableCell: passthrough('TableCellStub', 'td'),
  }
})

vi.mock('lucide-vue-next', async () => {
  const Icon = defineComponent({
    name: 'IconStub',
    setup() {
      return () => h('span')
    },
  })
  return {
    Users: Icon,
    Activity: Icon,
    TrendingUp: Icon,
    DollarSign: Icon,
    Key: Icon,
    Hash: Icon,
    Zap: Icon,
    Bell: Icon,
    AlertCircle: Icon,
    AlertTriangle: Icon,
    Info: Icon,
    Wrench: Icon,
    Loader2: Icon,
    Clock: Icon,
    Database: Icon,
    Shuffle: Icon,
    Gauge: Icon,
    Layers3: Icon,
    CalendarClock: Icon,
  }
})

const mountedApps: Array<{ app: App, root: HTMLElement }> = []

function mountDashboard() {
  const root = document.createElement('div')
  document.body.appendChild(root)
  const app = createApp(Dashboard)
  app.mount(root)
  mountedApps.push({ app, root })
  return root
}

async function settle() {
  for (let index = 0; index < 8; index += 1) {
    await Promise.resolve()
    await nextTick()
  }
}

beforeEach(() => {
  authStoreMock.canAccessAdmin = false
  authStoreMock.isAdmin = false
  authStoreMock.isAuditAdmin = false
  dashboardApiMocks.getStats.mockReset()
  dashboardApiMocks.getDailyStats.mockReset()
  admissionApiMocks.getAccountStatus.mockReset()
  dashboardApiMocks.getDailyStats.mockResolvedValue({
    daily_stats: [],
    model_summary: [],
    period: { start_date: '2026-05-01', end_date: '2026-05-15', days: 15 },
  })
  admissionApiMocks.getAccountStatus.mockResolvedValue({
    user_id: 'user-1',
    rules: [],
  })
})

afterEach(() => {
  for (const { app, root } of mountedApps.splice(0)) {
    app.unmount()
    root.remove()
  }
  document.body.innerHTML = ''
})

describe('Dashboard ordinary user wallet card', () => {
  it('renders the available quota from mocked stats', async () => {
    dashboardApiMocks.getStats.mockResolvedValue({
      stats: [
        { name: 'API 密钥', value: '0', subValue: '活跃 0', icon: 'Activity' },
        { name: '本月请求', value: '0', subValue: '今日 0', icon: 'Users' },
        {
          name: '钱包余额',
          value: '$110.00',
          subValue: '可用额度 $110.00',
          icon: 'DollarSign',
        },
        { name: '本月 Token', value: '0', subValue: '输入 0 / 输出 0', icon: 'Zap' },
      ],
      today: { requests: 0, tokens: 0, cost: 0 },
      cache_stats: { cache_creation_tokens: 0, cache_read_tokens: 0, total_cache_tokens: 0 },
      token_breakdown: { input: 0, output: 0, cache_creation: 0, cache_read: 0 },
      monthly_cost: 0,
    })

    const root = mountDashboard()
    await settle()

    expect(root.textContent).toContain('$110.00')
    expect(root.textContent).toContain('可用额度 $110.00')
  })
})

describe('Dashboard account admission status', () => {
  it('renders account usage and limits as a vertical list for ordinary users', async () => {
    dashboardApiMocks.getStats.mockResolvedValue({ stats: [] })
    admissionApiMocks.getAccountStatus.mockResolvedValue({
      user_id: 'user-1',
      rules: [
        {
          kind: 'request_count',
          available: true,
          status: 'available',
          limit: 100,
          used: 37,
          remaining: 63,
          window_seconds: 60,
          reset_at: '2026-08-17T12:01:00Z',
        },
        {
          kind: 'concurrent_requests',
          available: true,
          status: 'available',
          limit: 8,
          used: 2,
          remaining: 6,
        },
        {
          kind: 'usage_cost_usd',
          available: true,
          status: 'available',
          limit: 20,
          used: 4.25,
          remaining: 15.75,
          period: 'calendar_day',
          timezone: 'Asia/Hong_Kong',
          reset_at: '2026-08-17T16:00:00Z',
        },
      ],
    })

    const root = mountDashboard()
    await settle()

    expect(root.querySelectorAll('[data-admission-rule]')).toHaveLength(3)
    expect(root.textContent).toContain('每分钟请求')
    expect(root.textContent).toContain('37 / 100')
    expect(root.textContent).toContain('当前并发')
    expect(root.textContent).toContain('2 / 8')
    expect(root.textContent).toContain('今日额度')
    expect(root.textContent).toContain('$4.25 / $20.00')
  })

  it('does not request or render account admission status for administrators', async () => {
    authStoreMock.canAccessAdmin = true
    authStoreMock.isAdmin = true
    dashboardApiMocks.getStats.mockResolvedValue({ stats: [] })

    const root = mountDashboard()
    await settle()

    expect(admissionApiMocks.getAccountStatus).not.toHaveBeenCalled()
    expect(root.querySelector('[data-testid="account-admission-status"]')).toBeNull()
  })
})

describe('Dashboard refresh controls', () => {
  it('does not render or run automatic refresh', async () => {
    vi.useFakeTimers()
    dashboardApiMocks.getStats.mockResolvedValue({ stats: [] })

    try {
      const root = mountDashboard()
      await settle()

      expect(root.textContent).not.toContain('自动刷新')
      expect(dashboardApiMocks.getStats).toHaveBeenCalledTimes(1)
      expect(dashboardApiMocks.getDailyStats).toHaveBeenCalledTimes(1)

      await vi.advanceTimersByTimeAsync(60_000)
      await settle()

      expect(dashboardApiMocks.getStats).toHaveBeenCalledTimes(1)
      expect(dashboardApiMocks.getDailyStats).toHaveBeenCalledTimes(1)
    } finally {
      vi.useRealTimers()
    }
  })
})
