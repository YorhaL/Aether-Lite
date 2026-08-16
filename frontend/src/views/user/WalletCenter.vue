<template>
  <div class="space-y-6 pb-8">
    <div
      v-if="loading"
      class="py-16"
    >
      <LoadingState :message="legacyT('正在加载额度...')" />
    </div>

    <template v-else>
      <div class="flex items-center justify-between">
        <div>
          <h2 class="text-xl font-semibold">
            {{ legacyT('额度') }}
          </h2>
          <p class="mt-1 text-sm text-muted-foreground">
            {{ legacyT('用于内部 API 请求的可用额度。') }}
          </p>
        </div>
        <RefreshButton
          :loading="loading"
          @click="loadBalance"
        />
      </div>

      <Card class="max-w-xl p-6">
        <div class="text-xs uppercase tracking-wider text-muted-foreground">
          {{ legacyT('当前可用额度') }}
        </div>
        <div class="mt-3 text-4xl font-bold tabular-nums">
          {{ balance?.unlimited ? legacyT('无限制') : formatCurrency(availableBalance) }}
        </div>
        <div class="mt-3 flex items-center gap-2 text-sm text-muted-foreground">
          <span>{{ legacyT('状态') }}</span>
          <Badge :variant="walletStatusBadge(balance?.wallet?.status)">
            {{ legacyT(walletStatusLabel(balance?.wallet?.status)) }}
          </Badge>
        </div>
        <p
          v-if="loadError"
          class="mt-4 text-sm text-destructive"
        >
          {{ loadError }}
        </p>
      </Card>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Badge, Card, RefreshButton } from '@/components/ui'
import { LoadingState } from '@/components/common'
import { walletApi, type WalletBalanceResponse } from '@/api/wallet'
import { useI18n } from '@/i18n'
import { parseApiError } from '@/utils/errorParser'
import { formatWalletCurrency as formatCurrency, walletStatusBadge, walletStatusLabel } from '@/utils/walletDisplay'

const { legacyT } = useI18n()
const balance = ref<WalletBalanceResponse | null>(null)
const loading = ref(true)
const loadError = ref('')

const availableBalance = computed(() => balance.value?.wallet?.balance ?? balance.value?.balance ?? 0)

async function loadBalance() {
  loading.value = true
  loadError.value = ''
  try {
    balance.value = await walletApi.getBalance()
  } catch (error: unknown) {
    loadError.value = legacyT(parseApiError(error, '加载额度失败'))
  } finally {
    loading.value = false
  }
}

onMounted(loadBalance)
</script>
