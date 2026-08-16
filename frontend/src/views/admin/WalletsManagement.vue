<template>
  <div class="space-y-6 pb-8">
    <Card class="overflow-hidden">
      <div class="flex flex-col gap-3 border-b border-border/60 px-5 py-4 lg:flex-row lg:items-center lg:justify-between">
        <div>
          <h3 class="text-base font-semibold">
            额度管理
          </h3>
          <p class="mt-1 text-xs text-muted-foreground">
            查看内部 API 额度并进行管理员调整
          </p>
        </div>
        <RefreshButton
          :loading="loading"
          @click="loadWallets"
        />
      </div>

      <div class="space-y-4 p-5">
        <div class="flex flex-wrap items-center gap-2">
          <Select v-model="ownerTypeFilter">
            <SelectTrigger class="w-[160px]">
              <SelectValue placeholder="额度归属" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">
                全部归属
              </SelectItem>
              <SelectItem value="user">
                用户
              </SelectItem>
              <SelectItem value="api_key">
                独立密钥
              </SelectItem>
            </SelectContent>
          </Select>

          <Select v-model="statusFilter">
            <SelectTrigger class="w-[150px]">
              <SelectValue placeholder="状态" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">
                全部状态
              </SelectItem>
              <SelectItem value="active">
                正常
              </SelectItem>
              <SelectItem value="suspended">
                已冻结
              </SelectItem>
              <SelectItem value="closed">
                已关闭
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div class="overflow-hidden rounded-2xl border border-border/60 bg-background">
          <div class="overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>归属</TableHead>
                  <TableHead>类型</TableHead>
                  <TableHead>可用额度</TableHead>
                  <TableHead>累计消耗</TableHead>
                  <TableHead>状态</TableHead>
                  <TableHead class="text-right">
                    操作
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableRow
                  v-for="wallet in wallets"
                  :key="wallet.id"
                >
                  <TableCell>
                    <div class="font-medium">
                      {{ wallet.owner_name || '-' }}
                    </div>
                    <div class="mt-1 font-mono text-[11px] text-muted-foreground">
                      {{ wallet.id }}
                    </div>
                  </TableCell>
                  <TableCell>
                    {{ wallet.owner_type === 'api_key' ? '独立密钥' : '用户' }}
                  </TableCell>
                  <TableCell class="font-medium tabular-nums">
                    {{ wallet.unlimited ? '无限制' : formatWalletCurrency(wallet.balance) }}
                  </TableCell>
                  <TableCell class="tabular-nums text-muted-foreground">
                    {{ formatWalletCurrency(wallet.total_consumed) }}
                  </TableCell>
                  <TableCell>
                    <Badge :variant="walletStatusBadge(wallet.status)">
                      {{ walletStatusLabel(wallet.status) }}
                    </Badge>
                  </TableCell>
                  <TableCell class="text-right">
                    <Button
                      size="sm"
                      variant="outline"
                      :disabled="loadingDetailId === wallet.id"
                      @click="openWallet(wallet)"
                    >
                      {{ loadingDetailId === wallet.id ? '加载中...' : '调整额度' }}
                    </Button>
                  </TableCell>
                </TableRow>
                <TableRow v-if="!loading && wallets.length === 0">
                  <TableCell
                    colspan="6"
                    class="py-12"
                  >
                    <EmptyState
                      title="暂无额度记录"
                      description="当前筛选条件下没有钱包"
                    />
                  </TableCell>
                </TableRow>
              </TableBody>
            </Table>
          </div>
        </div>

        <Pagination
          :current="page"
          :total="total"
          :page-size="pageSize"
          @update:current="handlePageChange"
          @update:page-size="handlePageSizeChange"
        />
      </div>
    </Card>

    <WalletOpsDrawer
      :open="Boolean(selectedWallet)"
      :wallet="selectedWallet"
      :owner-name="selectedWallet?.owner_name || ''"
      :owner-subtitle="selectedWallet?.owner_type === 'api_key' ? '独立密钥' : '用户'"
      context-label="额度详情"
      @close="selectedWallet = null"
      @changed="handleWalletChanged"
    />
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import {
  Badge,
  Button,
  Card,
  Pagination,
  RefreshButton,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui'
import { EmptyState } from '@/components/common'
import { adminWalletApi, type AdminWallet } from '@/api/admin-wallets'
import WalletOpsDrawer from '@/features/wallet/components/WalletOpsDrawer.vue'
import { useToast } from '@/composables/useToast'
import { parseApiError } from '@/utils/errorParser'
import { formatWalletCurrency, walletStatusBadge, walletStatusLabel } from '@/utils/walletDisplay'

const { error: showError } = useToast()
const wallets = ref<AdminWallet[]>([])
const total = ref(0)
const page = ref(1)
const pageSize = ref(20)
const ownerTypeFilter = ref<'all' | 'user' | 'api_key'>('all')
const statusFilter = ref('all')
const loading = ref(false)
const loadingDetailId = ref<string | null>(null)
const selectedWallet = ref<AdminWallet | null>(null)

async function loadWallets() {
  loading.value = true
  try {
    const response = await adminWalletApi.listWallets({
      owner_type: ownerTypeFilter.value === 'all' ? undefined : ownerTypeFilter.value,
      status: statusFilter.value === 'all' ? undefined : statusFilter.value,
      limit: pageSize.value,
      offset: (page.value - 1) * pageSize.value,
    })
    wallets.value = response.items
    total.value = response.total
  } catch (error: unknown) {
    showError(parseApiError(error, '加载额度列表失败'))
  } finally {
    loading.value = false
  }
}

async function openWallet(wallet: AdminWallet) {
  loadingDetailId.value = wallet.id
  try {
    selectedWallet.value = await adminWalletApi.getWalletDetail(wallet.id)
  } catch (error: unknown) {
    showError(parseApiError(error, '加载额度详情失败'))
  } finally {
    loadingDetailId.value = null
  }
}

function handlePageChange(nextPage: number) {
  page.value = nextPage
  void loadWallets()
}

function handlePageSizeChange(size: number) {
  pageSize.value = size
  page.value = 1
  void loadWallets()
}

async function handleWalletChanged() {
  await loadWallets()
  if (selectedWallet.value) {
    selectedWallet.value = await adminWalletApi.getWalletDetail(selectedWallet.value.id)
  }
}

watch([ownerTypeFilter, statusFilter], () => {
  page.value = 1
  void loadWallets()
})

onMounted(loadWallets)
</script>
