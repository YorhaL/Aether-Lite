<template>
  <div class="space-y-6 px-4 sm:px-6 lg:px-0">
    <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
      <div>
        <h1 class="text-lg font-semibold">
          用户统计
        </h1>
        <p class="text-xs text-muted-foreground">
          查看用户排行榜与使用趋势
        </p>
      </div>
      <div class="flex flex-wrap items-center gap-3">
        <TimeRangePicker
          v-model="timeRange"
          :allow-hourly="true"
        />
        <Select
          v-model="selectedUserId"
        >
          <SelectTrigger class="h-8 text-xs w-52">
            <SelectValue placeholder="选择用户" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="user in users"
              :key="user.id"
              :value="user.id"
            >
              {{ user.username || user.email }}
            </SelectItem>
          </SelectContent>
        </Select>
        <Select
          v-model="compareUserId"
        >
          <SelectTrigger class="h-8 text-xs w-52">
            <SelectValue placeholder="对比用户（可选）" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__none__">
              不对比
            </SelectItem>
            <SelectItem
              v-for="user in users"
              :key="`compare-${user.id}`"
              :value="user.id"
            >
              {{ user.username || user.email }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>
    </div>

    <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
      <LeaderboardTable
        title="用户排行榜"
        :items="leaderboard"
        :metric="metric"
        :loading="leaderboardLoading"
        @update:metric="metric = $event"
      >
        <template #pagination>
          <Pagination
            v-if="leaderboardTotal > 0"
            :current="leaderboardPage"
            :total="leaderboardTotal"
            :page-size="leaderboardPageSize"
            :page-size-options="leaderboardPageSizeOptions"
            @update:current="leaderboardPage = $event"
            @update:page-size="leaderboardPageSize = $event"
          />
        </template>
      </LeaderboardTable>

      <Card class="p-4 space-y-3">
        <h3 class="text-sm font-semibold">
          用户摘要
        </h3>
        <div
          v-if="summaryLoading"
          class="p-6"
        >
          <LoadingState />
        </div>
        <div
          v-else
          class="space-y-4"
        >
          <div class="grid grid-cols-2 gap-3 text-sm">
            <div>
              <div class="text-xs text-muted-foreground">
                请求数
              </div>
              <div class="font-semibold">
                {{ userSummary?.total_requests ?? 0 }}
              </div>
            </div>
            <div>
              <div class="text-xs text-muted-foreground">
                Tokens
              </div>
              <div class="font-semibold">
                {{ formatTokens(userSummary?.total_tokens ?? 0) }}
              </div>
            </div>
            <div>
              <div class="text-xs text-muted-foreground">
                成本
              </div>
              <div class="font-semibold">
                {{ formatCurrency(userSummary?.total_cost ?? 0) }}
              </div>
            </div>
            <div>
              <div class="text-xs text-muted-foreground">
                错误率
              </div>
              <div class="font-semibold">
                {{ userSummary?.error_rate ?? 0 }}%
              </div>
            </div>
          </div>

          <div class="grid gap-4 border-t border-border/60 pt-4 xl:grid-cols-2">
            <section class="min-w-0 space-y-2">
              <div class="flex items-center justify-between gap-2">
                <h4 class="text-xs font-medium">
                  Model 用量
                </h4>
                <span class="text-[11px] text-muted-foreground">
                  {{ modelUsage.length }} 种
                </span>
              </div>
              <div
                v-if="modelUsage.length === 0"
                class="rounded-md border border-dashed px-3 py-4 text-center text-xs text-muted-foreground"
              >
                当前范围内暂无 Model 用量
              </div>
              <div
                v-else
                class="max-h-64 space-y-2 overflow-y-auto pr-1"
              >
                <div
                  v-for="item in modelUsage"
                  :key="item.model"
                  class="rounded-md border border-border/60 p-2.5"
                >
                  <div class="flex items-start justify-between gap-2 text-xs">
                    <span
                      class="min-w-0 truncate font-medium"
                      :title="item.model"
                    >
                      {{ item.model }}
                    </span>
                    <span class="shrink-0 text-muted-foreground">
                      {{ usageShare(item.total_tokens) }}
                    </span>
                  </div>
                  <div class="mt-2 h-1.5 overflow-hidden rounded-full bg-muted">
                    <div
                      class="h-full rounded-full bg-primary"
                      :style="{ width: usageShare(item.total_tokens) }"
                    />
                  </div>
                  <div class="mt-2 flex flex-wrap justify-between gap-x-2 text-[11px] text-muted-foreground">
                    <span>{{ item.request_count }} 次</span>
                    <span>{{ formatTokens(item.total_tokens) }}</span>
                    <span>{{ formatCurrency(item.total_cost) }}</span>
                  </div>
                </div>
              </div>
            </section>

            <section class="min-w-0 space-y-2">
              <div class="flex items-center justify-between gap-2">
                <h4 class="text-xs font-medium">
                  Provider 用量
                </h4>
                <span class="text-[11px] text-muted-foreground">
                  {{ providerUsage.length }} 个
                </span>
              </div>
              <div
                v-if="providerUsage.length === 0"
                class="rounded-md border border-dashed px-3 py-4 text-center text-xs text-muted-foreground"
              >
                当前范围内暂无 Provider 用量
              </div>
              <div
                v-else
                class="max-h-64 space-y-2 overflow-y-auto pr-1"
              >
                <div
                  v-for="item in providerUsage"
                  :key="item.provider_key || item.provider_id || item.provider"
                  class="rounded-md border border-border/60 p-2.5"
                >
                  <div class="flex items-start justify-between gap-2 text-xs">
                    <span
                      class="min-w-0 truncate font-medium"
                      :title="item.provider"
                    >
                      {{ item.provider }}
                    </span>
                    <span class="shrink-0 text-muted-foreground">
                      {{ usageShare(item.total_tokens) }}
                    </span>
                  </div>
                  <div class="mt-2 h-1.5 overflow-hidden rounded-full bg-muted">
                    <div
                      class="h-full rounded-full bg-emerald-500/75"
                      :style="{ width: usageShare(item.total_tokens) }"
                    />
                  </div>
                  <div class="mt-2 flex flex-wrap justify-between gap-x-2 text-[11px] text-muted-foreground">
                    <span>{{ item.request_count }} 次</span>
                    <span>{{ formatTokens(item.total_tokens) }}</span>
                    <span>{{ formatCurrency(item.total_cost) }}</span>
                  </div>
                </div>
              </div>
            </section>
          </div>
        </div>
      </Card>
    </div>

    <Card class="p-4 space-y-4">
      <h3 class="text-sm font-semibold">
        用户使用趋势
      </h3>
      <div
        v-if="seriesLoading"
        class="p-6"
      >
        <LoadingState />
      </div>
      <div
        v-else
        class="h-[280px]"
      >
        <LineChart :data="seriesChartData" />
      </div>
    </Card>

    <Card
      v-if="comparisonSeries.length > 0"
      class="p-4 space-y-4"
    >
      <h3 class="text-sm font-semibold">
        用户对比趋势
      </h3>
      <div class="h-[280px]">
        <LineChart :data="comparisonChartData" />
      </div>
    </Card>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { Card, Pagination, Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui'
import LineChart from '@/components/charts/LineChart.vue'
import { LoadingState, TimeRangePicker } from '@/components/common'
import { LeaderboardTable } from '@/components/stats'
import { adminApi, type LeaderboardItem } from '@/api/admin'
import { usersApi, type User } from '@/api/users'
import { usageApi, type UsageByModel, type UsageByProvider } from '@/api/usage'
import { formatCurrency, formatTokens } from '@/utils/format'
import { getDateRangeFromPeriod } from '@/features/usage/composables'
import type { DateRangeParams } from '@/features/usage/types'

const timeRange = ref<DateRangeParams>(getDateRangeFromPeriod('today'))
const metric = ref<'requests' | 'tokens' | 'cost'>('requests')

const users = ref<User[]>([])
const selectedUserId = ref<string | null>(null)
const compareUserId = ref<string>('__none__')

const leaderboard = ref<LeaderboardItem[]>([])
const leaderboardPage = ref(1)
const leaderboardPageSize = ref(10)
const leaderboardTotal = ref(0)
const leaderboardPageSizeOptions = [10, 20, 50, 100]
const leaderboardLoading = ref(false)

interface UsageSummary {
  total_requests: number
  total_tokens: number
  total_cost: number
  error_rate: number
}

interface TimeSeriesItem {
  date: string
  total_cost: number
}

const userSummary = ref<UsageSummary | null>(null)
const modelUsage = ref<UsageByModel[]>([])
const providerUsage = ref<UsageByProvider[]>([])
const summaryLoading = ref(false)

const series = ref<TimeSeriesItem[]>([])
const comparisonSeries = ref<TimeSeriesItem[]>([])
const seriesLoading = ref(false)
let leaderboardRequestId = 0
let summaryRequestId = 0
let seriesRequestId = 0
let leaderboardLoadPromise: Promise<void> | null = null
let hasPendingLeaderboardLoad = false
let leaderboardDebounceTimer: ReturnType<typeof setTimeout> | null = null
let userPanelsLoadPromise: Promise<void> | null = null
let hasPendingUserPanelsLoad = false
let userPanelsDebounceTimer: ReturnType<typeof setTimeout> | null = null

function buildTimeRangeParams() {
  return {
    start_date: timeRange.value.start_date,
    end_date: timeRange.value.end_date,
    preset: timeRange.value.preset,
    timezone: timeRange.value.timezone,
    tz_offset_minutes: timeRange.value.tz_offset_minutes,
    granularity: timeRange.value.granularity || 'day'
  }
}

async function loadUsers() {
  users.value = await usersApi.getAllUsers()
  if (!selectedUserId.value && users.value.length > 0) {
    selectedUserId.value = users.value[0].id
  }
}

async function loadLeaderboard() {
  if (leaderboardLoadPromise) {
    hasPendingLeaderboardLoad = true
    return leaderboardLoadPromise
  }
  leaderboardLoadPromise = (async () => {
  const requestId = ++leaderboardRequestId
  leaderboardLoading.value = true
  try {
    const response = await adminApi.getLeaderboardUsers({
      ...buildTimeRangeParams(),
      metric: metric.value,
      limit: leaderboardPageSize.value,
      offset: (leaderboardPage.value - 1) * leaderboardPageSize.value,
      include_inactive: true
    })
    if (requestId !== leaderboardRequestId) return
    leaderboard.value = response.items
    leaderboardTotal.value = response.total
    if (response.items.length === 0 && response.total > 0 && leaderboardPage.value > 1) {
      leaderboardPage.value = 1
    }
  } finally {
    if (requestId === leaderboardRequestId) {
      leaderboardLoading.value = false
    }
  }
  })().finally(() => {
    leaderboardLoadPromise = null
    if (hasPendingLeaderboardLoad) {
      hasPendingLeaderboardLoad = false
      void loadLeaderboard()
    }
  })
  return leaderboardLoadPromise
}

async function loadSummary() {
  if (!selectedUserId.value) return
  const requestId = ++summaryRequestId
  summaryLoading.value = true
  try {
    const filters = {
      ...buildTimeRangeParams(),
      user_id: selectedUserId.value
    }
    const [summary, models, providers] = await Promise.all([
      usageApi.getUsageStats(filters),
      usageApi.getUsageByModel({ ...filters, limit: 100 }),
      usageApi.getUsageByProvider({ ...filters, limit: 100 })
    ])
    if (requestId !== summaryRequestId) return
    userSummary.value = summary
    modelUsage.value = [...models].sort((left, right) => right.total_tokens - left.total_tokens)
    providerUsage.value = [...providers].sort((left, right) => right.total_tokens - left.total_tokens)
  } finally {
    if (requestId === summaryRequestId) {
      summaryLoading.value = false
    }
  }
}

function usageShare(tokens: number): string {
  const total = userSummary.value?.total_tokens ?? 0
  if (total <= 0 || tokens <= 0) {
    return '0%'
  }
  return `${Math.min(100, Math.max(0, tokens / total * 100)).toFixed(1)}%`
}

async function loadSeries() {
  if (!selectedUserId.value) return
  const requestId = ++seriesRequestId
  seriesLoading.value = true
  try {
    const baseParams = {
      ...buildTimeRangeParams(),
      user_id: selectedUserId.value
    }
    const shouldCompare = Boolean(compareUserId.value && compareUserId.value !== '__none__')
    const comparePromise: Promise<TimeSeriesItem[]> = shouldCompare
      ? adminApi.getTimeSeries({
        ...buildTimeRangeParams(),
        user_id: compareUserId.value
      })
      : Promise.resolve([])

    const [primarySeries, compareSeries] = await Promise.all([
      adminApi.getTimeSeries(baseParams),
      comparePromise
    ])

    if (requestId !== seriesRequestId) return
    series.value = primarySeries
    comparisonSeries.value = compareSeries
  } finally {
    if (requestId === seriesRequestId) {
      seriesLoading.value = false
    }
  }
}

async function loadUserPanels() {
  if (userPanelsLoadPromise) {
    hasPendingUserPanelsLoad = true
    return userPanelsLoadPromise
  }
  userPanelsLoadPromise = Promise.all([loadSummary(), loadSeries()])
    .then(() => undefined)
    .finally(() => {
      userPanelsLoadPromise = null
      if (hasPendingUserPanelsLoad) {
        hasPendingUserPanelsLoad = false
        void loadUserPanels()
      }
    })
  return userPanelsLoadPromise
}

const seriesChartData = computed(() => ({
  labels: series.value.map(item => item.date),
  datasets: [
    {
      label: '成本',
      data: series.value.map(item => item.total_cost),
      borderColor: 'rgb(59, 130, 246)',
      tension: 0.25,
      pointRadius: 2
    }
  ]
}))

const comparisonChartData = computed(() => ({
  labels: series.value.map(item => item.date),
  datasets: [
    {
      label: '当前用户',
      data: series.value.map(item => item.total_cost),
      borderColor: 'rgb(59, 130, 246)',
      tension: 0.25,
      pointRadius: 2
    },
    {
      label: '对比用户',
      data: comparisonSeries.value.map(item => item.total_cost),
      borderColor: 'rgb(234, 179, 8)',
      tension: 0.25,
      pointRadius: 2
    }
  ]
}))

function scheduleLeaderboardLoad() {
  if (leaderboardDebounceTimer) {
    clearTimeout(leaderboardDebounceTimer)
  }
  leaderboardDebounceTimer = setTimeout(() => {
    leaderboardDebounceTimer = null
    void loadLeaderboard()
  }, 120)
}

function scheduleUserPanelsLoad() {
  if (userPanelsDebounceTimer) {
    clearTimeout(userPanelsDebounceTimer)
  }
  userPanelsDebounceTimer = setTimeout(() => {
    userPanelsDebounceTimer = null
    void loadUserPanels()
  }, 120)
}

function resetLeaderboardPage() {
  if (leaderboardPage.value === 1) {
    return
  }
  leaderboardPage.value = 1
}

watch([timeRange, metric], () => {
  resetLeaderboardPage()
  scheduleLeaderboardLoad()
}, { deep: true })
watch([leaderboardPage, leaderboardPageSize], scheduleLeaderboardLoad)
watch([timeRange, selectedUserId, compareUserId], scheduleUserPanelsLoad, { deep: true })

onMounted(async () => {
  await Promise.all([
    loadLeaderboard(),
    loadUsers()
  ])
})

onUnmounted(() => {
  if (leaderboardDebounceTimer) {
    clearTimeout(leaderboardDebounceTimer)
    leaderboardDebounceTimer = null
  }
  if (userPanelsDebounceTimer) {
    clearTimeout(userPanelsDebounceTimer)
    userPanelsDebounceTimer = null
  }
  hasPendingLeaderboardLoad = false
  hasPendingUserPanelsLoad = false
  leaderboardLoadPromise = null
  userPanelsLoadPromise = null
  leaderboardRequestId += 1
  summaryRequestId += 1
  seriesRequestId += 1
})
</script>
