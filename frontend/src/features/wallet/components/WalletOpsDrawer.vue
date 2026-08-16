<template>
  <Teleport to="body">
    <Transition name="drawer">
      <div
        v-if="open && localWallet"
        class="fixed inset-0 z-[80] flex justify-end"
      >
        <div
          class="absolute inset-0 bg-black/35"
          @click="handleClose"
        />

        <div class="drawer-panel relative h-full w-full overflow-y-auto border-l border-border bg-background shadow-2xl sm:w-[520px] sm:max-w-[95vw]">
          <div class="sticky top-0 z-10 flex items-start justify-between gap-3 border-b border-border bg-background px-5 py-4">
            <div>
              <h3 class="text-lg font-semibold">
                {{ contextLabel }}
              </h3>
              <p class="text-xs text-muted-foreground">
                {{ ownerName || '-' }} <span v-if="ownerSubtitle">· {{ ownerSubtitle }}</span>
              </p>
            </div>
            <Button
              variant="ghost"
              size="icon"
              title="关闭"
              @click="handleClose"
            >
              <X class="h-4 w-4" />
            </Button>
          </div>

          <div class="space-y-5 p-5">
            <Card class="p-5">
              <div class="text-xs uppercase tracking-wider text-muted-foreground">
                当前可用额度
              </div>
              <div class="mt-2 text-3xl font-bold tabular-nums">
                {{ localWallet.unlimited ? '无限制' : formatWalletCurrency(localWallet.balance) }}
              </div>
              <Badge
                :variant="walletStatusBadge(localWallet.status)"
                class="mt-3"
              >
                {{ walletStatusLabel(localWallet.status) }}
              </Badge>
            </Card>

            <Card class="space-y-4 p-5">
              <div>
                <h4 class="font-medium">
                  调整额度
                </h4>
                <p class="mt-1 text-xs text-muted-foreground">
                  正数增加额度，负数扣减额度。
                </p>
              </div>

              <div class="space-y-2">
                <Label for="wallet-adjust-amount">额度 (USD)</Label>
                <Input
                  id="wallet-adjust-amount"
                  :model-value="amount ?? ''"
                  type="number"
                  step="0.01"
                  placeholder="例如 10 或 -5"
                  @update:model-value="amount = parseNumberInput($event, { allowFloat: true })"
                />
              </div>

              <div class="space-y-2">
                <Label for="wallet-adjust-description">说明</Label>
                <Input
                  id="wallet-adjust-description"
                  v-model="description"
                  placeholder="可选的内部备注"
                />
              </div>

              <div class="flex justify-end gap-2">
                <Button
                  variant="outline"
                  @click="handleClose"
                >
                  取消
                </Button>
                <Button
                  :disabled="submitting || !amount"
                  @click="submitAdjustment"
                >
                  {{ submitting ? '处理中...' : '确认调整' }}
                </Button>
              </div>
            </Card>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { X } from 'lucide-vue-next'
import { Badge, Button, Card, Input, Label } from '@/components/ui'
import { adminWalletApi, type AdminWallet } from '@/api/admin-wallets'
import { useToast } from '@/composables/useToast'
import { parseApiError } from '@/utils/errorParser'
import { parseNumberInput } from '@/utils/form'
import { formatWalletCurrency, walletStatusBadge, walletStatusLabel } from '@/utils/walletDisplay'

const props = withDefaults(defineProps<{
  open: boolean
  wallet: AdminWallet | null
  ownerName?: string
  ownerSubtitle?: string
  contextLabel?: string
}>(), {
  ownerName: '',
  ownerSubtitle: '',
  contextLabel: '额度详情',
})

const emit = defineEmits<{
  close: []
  changed: []
}>()

const { success, error: showError } = useToast()
const localWallet = ref<AdminWallet | null>(null)
const amount = ref<number | undefined>()
const description = ref('')
const submitting = ref(false)

watch(
  () => [props.open, props.wallet] as const,
  ([open, wallet]) => {
    if (!open || !wallet) return
    localWallet.value = { ...wallet }
    amount.value = undefined
    description.value = ''
  },
  { immediate: true },
)

function handleClose() {
  emit('close')
}

async function submitAdjustment() {
  if (!localWallet.value || !amount.value) return
  submitting.value = true
  try {
    const response = await adminWalletApi.adjustWallet(localWallet.value.id, {
      amount_usd: amount.value,
      description: description.value || undefined,
    })
    localWallet.value = response.wallet
    amount.value = undefined
    description.value = ''
    success('额度已调整')
    emit('changed')
  } catch (error: unknown) {
    showError(parseApiError(error, '调整额度失败'))
  } finally {
    submitting.value = false
  }
}
</script>

<style scoped>
.drawer-enter-active,
.drawer-leave-active {
  transition: opacity 0.2s ease;
}

.drawer-enter-active .drawer-panel,
.drawer-leave-active .drawer-panel {
  transition: transform 0.25s ease;
}

.drawer-enter-from,
.drawer-leave-to {
  opacity: 0;
}

.drawer-enter-from .drawer-panel,
.drawer-leave-to .drawer-panel {
  transform: translateX(100%);
}
</style>
