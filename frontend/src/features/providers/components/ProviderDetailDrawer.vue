<template>
  <Teleport to="body">
    <Transition
      name="drawer"
      appear
    >
      <div
        v-if="open && (loading || provider)"
        class="fixed inset-0 z-50 flex justify-end"
        @click.self="handleBackdropClick"
      >
        <div
          class="absolute inset-0 bg-black/30"
          @click="handleBackdropClick"
        />

        <Card class="drawer-panel relative h-full w-full overflow-y-auto rounded-none shadow-2xl sm:w-[700px] sm:max-w-[90vw]">
          <div
            v-if="loading"
            class="flex items-center justify-center py-12"
          >
            <Loader2 class="h-8 w-8 animate-spin text-primary" />
          </div>

          <template v-else-if="provider">
            <ProviderDetailHeader
              :provider="provider"
              :endpoints="endpoints"
              :loading-provider-endpoints="loadingProviderEndpoints"
              :has-failover-rules="hasFailoverRules"
              @open-failover-rules="failoverRulesDialogOpen = true"
              @edit="$emit('edit', $event)"
              @toggle-status="$emit('toggleStatus', $event)"
              @close="handleClose"
              @edit-endpoint="handleEditEndpoint"
              @add-endpoint="endpointDialogOpen = true"
            />

            <div class="space-y-6 p-4 sm:p-6">
              <Card class="overflow-hidden">
                <div class="flex items-center justify-between border-b border-border/60 p-4">
                  <h3 class="text-sm font-semibold">
                    {{ legacyT('密钥管理') }}
                  </h3>
                  <div class="flex items-center gap-2">
                    <Button
                      v-if="endpoints.length > 0"
                      variant="outline"
                      size="sm"
                      class="h-9"
                      @click="handleAddKey"
                    >
                      <Plus class="mr-1.5 h-3.5 w-3.5" />
                      {{ legacyT('添加密钥') }}
                    </Button>
                  </div>
                </div>

                <div
                  v-if="loadingProviderKeys"
                  class="flex items-center justify-center gap-2 py-12 text-sm text-muted-foreground"
                >
                  <Loader2 class="h-4 w-4 animate-spin" />
                  {{ legacyT('正在加载密钥') }}
                </div>

                <div
                  v-else-if="providerKeys.length > 0"
                  class="divide-y divide-border/40"
                >
                  <div
                    v-for="key in providerKeys"
                    :key="key.id"
                    class="flex items-center justify-between gap-3 px-4 py-3 transition-colors hover:bg-muted/30"
                    :class="{ 'opacity-40 bg-muted/20': !key.is_active }"
                  >
                    <ProviderKeyIdentityBlock
                      :api-key="key"
                      :masked-secret-label="key.api_key_masked || '••••••••'"
                      @copy-name="copyToClipboard"
                      @copy-full-key="copyFullKey(key)"
                    />
                    <ProviderKeyActionCluster
                      :api-key="key"
                      :recoverable="isKeyRecoverable(key)"
                      :recover-title="legacyT('恢复密钥健康状态')"
                      :circuit-breaker-title="legacyT('密钥当前处于熔断状态')"
                      :health-score-bar-class="getHealthScoreBarColor(key.health_score || 0)"
                      :health-score-text-class="getHealthScoreColor(key.health_score || 0)"
                      :toggling="togglingKeyId === key.id"
                      @recover="handleRecoverKey(key)"
                      @permissions="handleKeyPermissions(key)"
                      @edit="handleEditKey(key)"
                      @toggle-active="toggleKeyActive(key)"
                      @delete="handleDeleteKey(key)"
                    />
                  </div>

                  <div
                    v-if="totalKeyPages > 1"
                    class="flex items-center justify-between px-4 py-3"
                  >
                    <span class="text-xs text-muted-foreground">
                      {{ currentKeyPage }} / {{ totalKeyPages }}
                    </span>
                    <div class="flex gap-1">
                      <Button
                        variant="outline"
                        size="icon"
                        class="h-7 w-7"
                        :disabled="currentKeyPage <= 1 || loadingProviderKeys"
                        @click="loadProviderKeysPage(currentKeyPage - 1)"
                      >
                        <ChevronLeft class="h-3.5 w-3.5" />
                      </Button>
                      <Button
                        variant="outline"
                        size="icon"
                        class="h-7 w-7"
                        :disabled="currentKeyPage >= totalKeyPages || loadingProviderKeys"
                        @click="loadProviderKeysPage(currentKeyPage + 1)"
                      >
                        <ChevronRight class="h-3.5 w-3.5" />
                      </Button>
                    </div>
                  </div>
                </div>

                <div
                  v-else
                  class="py-12 text-center text-sm text-muted-foreground"
                >
                  {{ endpoints.length > 0 ? legacyT('暂无密钥') : legacyT('请先添加 API 端点') }}
                </div>
              </Card>

              <ModelsTab
                :key="`models-${provider.id}`"
                :provider="provider"
                :models="providerModels"
                :endpoints="endpoints"
                :provider-keys="providerKeys"
                :loading="loadingProviderModels"
                @edit-model="handleEditModel"
                @batch-assign="batchAssignDialogOpen = true"
                @refresh="loadAllProviderData"
              />

              <ModelMappingTab
                ref="modelMappingTabRef"
                :key="`mapping-${provider.id}`"
                :provider="provider"
                :endpoints="endpoints"
                :provider-keys="providerKeys"
                :models="providerModels"
                :mapping-preview="providerMappingPreview"
                :loading="loadingProviderMappingPreview"
                @refresh="handleDataChanged"
              />
            </div>
          </template>
        </Card>
      </div>
    </Transition>
  </Teleport>

  <EndpointFormDialog
    v-if="provider && open && endpointDialogOpen"
    v-model="endpointDialogOpen"
    :provider="provider"
    :endpoints="endpoints"
    @endpoint-created="handleDataChanged"
    @endpoint-updated="handleDataChanged"
  />

  <KeyFormDialog
    v-if="open && keyFormDialogOpen"
    :open="keyFormDialogOpen"
    :endpoint="currentEndpoint"
    :editing-key="editingKey"
    :provider-id="provider?.id || null"
    :available-api-formats="availableKeyApiFormats"
    @close="keyFormDialogOpen = false"
    @saved="handleDataChanged"
  />

  <KeyAllowedModelsEditDialog
    v-if="open && keyPermissionsDialogOpen"
    :open="keyPermissionsDialogOpen"
    :api-key="editingKey"
    :provider-id="providerId || ''"
    @close="keyPermissionsDialogOpen = false"
    @saved="handleDataChanged"
  />

  <AlertDialog
    v-if="open && deleteKeyConfirmOpen"
    :model-value="deleteKeyConfirmOpen"
    :title="legacyT('删除密钥')"
    :description="deleteKeyConfirmDescription"
    :confirm-text="legacyT('删除')"
    :cancel-text="legacyT('取消')"
    type="danger"
    @update:model-value="deleteKeyConfirmOpen = $event"
    @confirm="confirmDeleteKey"
    @cancel="deleteKeyConfirmOpen = false"
  />

  <ProviderModelFormDialog
    v-if="open && modelFormDialogOpen && provider"
    :open="modelFormDialogOpen"
    :provider-id="provider.id"
    :provider-name="provider.name"
    :editing-model="editingModel"
    @update:open="modelFormDialogOpen = $event"
    @saved="handleModelSaved"
  />

  <BatchAssignModelsDialog
    v-if="open && batchAssignDialogOpen && provider"
    :open="batchAssignDialogOpen"
    :provider-id="provider.id"
    :provider-name="provider.name"
    @update:open="batchAssignDialogOpen = $event"
    @changed="handleDataChanged"
  />

  <FailoverRulesDialog
    v-if="open && failoverRulesDialogOpen"
    :open="failoverRulesDialogOpen"
    :provider="provider"
    @update:open="failoverRulesDialogOpen = $event"
    @saved="handleDataChanged"
  />
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ChevronLeft, ChevronRight, Loader2, Plus } from 'lucide-vue-next'
import Button from '@/components/ui/button.vue'
import Card from '@/components/ui/card.vue'
import AlertDialog from '@/components/common/AlertDialog.vue'
import { useClipboard } from '@/composables/useClipboard'
import { useEscapeKey } from '@/composables/useEscapeKey'
import { useToast } from '@/composables/useToast'
import { useI18n } from '@/i18n'
import { parseApiError } from '@/utils/errorParser'
import {
  API_FORMAT_ORDER,
  deleteEndpointKey,
  getProvider,
  getProviderEndpoints,
  getProviderKeysPage,
  getProviderMappingPreview,
  getProviderModels,
  recoverKeyHealth,
  revealEndpointKey,
  sortApiFormats,
  updateProviderKey,
  type EndpointAPIKey,
  type Model,
  type ProviderEndpoint,
  type ProviderMappingPreviewResponse,
  type ProviderWithEndpointsSummary,
} from '@/api/endpoints'
import BatchAssignModelsDialog from './BatchAssignModelsDialog.vue'
import EndpointFormDialog from './EndpointFormDialog.vue'
import FailoverRulesDialog from './FailoverRulesDialog.vue'
import KeyAllowedModelsEditDialog from './KeyAllowedModelsEditDialog.vue'
import KeyFormDialog from './KeyFormDialog.vue'
import ModelsTab from './provider-tabs/ModelsTab.vue'
import ProviderDetailHeader from './ProviderDetailHeader.vue'
import ProviderKeyActionCluster from './ProviderKeyActionCluster.vue'
import ProviderKeyIdentityBlock from './ProviderKeyIdentityBlock.vue'
import ProviderModelFormDialog from './ProviderModelFormDialog.vue'
import ModelMappingTab from './provider-tabs/ModelMappingTab.vue'

interface Props {
  providerId: string | null
  open: boolean
  initialProvider?: ProviderWithEndpointsSummary | null
}

const props = defineProps<Props>()
const emit = defineEmits<{
  (e: 'update:open', value: boolean): void
  (e: 'edit', provider: ProviderWithEndpointsSummary): void
  (e: 'toggleStatus', provider: ProviderWithEndpointsSummary): void
  (e: 'refresh'): void
}>()

const { legacyT, locale } = useI18n()
const { error: showError, success: showSuccess } = useToast()
const { copyToClipboard } = useClipboard()

const loading = ref(false)
const provider = ref<ProviderWithEndpointsSummary | null>(null)
const endpoints = ref<ProviderEndpoint[]>([])
const providerKeys = ref<EndpointAPIKey[]>([])
const providerModels = ref<Model[]>([])
const providerMappingPreview = ref<ProviderMappingPreviewResponse | null>(null)
const loadingProviderEndpoints = ref(false)
const loadingProviderKeys = ref(false)
const loadingProviderModels = ref(false)
const loadingProviderMappingPreview = ref(false)
const providerKeysTotal = ref(0)
const currentKeyPage = ref(1)
const keyPageSize = 4

const endpointDialogOpen = ref(false)
const keyFormDialogOpen = ref(false)
const keyPermissionsDialogOpen = ref(false)
const deleteKeyConfirmOpen = ref(false)
const modelFormDialogOpen = ref(false)
const batchAssignDialogOpen = ref(false)
const failoverRulesDialogOpen = ref(false)
const currentEndpoint = ref<ProviderEndpoint | null>(null)
const editingKey = ref<EndpointAPIKey | null>(null)
const keyToDelete = ref<EndpointAPIKey | null>(null)
const editingModel = ref<Model | null>(null)
const togglingKeyId = ref<string | null>(null)
const modelMappingTabRef = ref<InstanceType<typeof ModelMappingTab> | null>(null)
const revealedKeys = new Map<string, string>()

const totalKeyPages = computed(() => Math.max(1, Math.ceil(providerKeysTotal.value / keyPageSize)))

const availableKeyApiFormats = computed(() => {
  const formats = new Set(provider.value?.api_formats || [])
  endpoints.value.forEach(endpoint => formats.add(endpoint.api_format))
  return sortApiFormats([...formats].filter(Boolean))
})

const hasFailoverRules = computed(() => {
  const rules = provider.value?.failover_rules
  if (!rules) return false
  return Object.values(rules).some(value => Array.isArray(value) ? value.length > 0 : value === true || typeof value === 'number')
})

const hasBlockingDialogOpen = computed(() =>
  endpointDialogOpen.value
  || keyFormDialogOpen.value
  || keyPermissionsDialogOpen.value
  || deleteKeyConfirmOpen.value
  || modelFormDialogOpen.value
  || batchAssignDialogOpen.value
  || failoverRulesDialogOpen.value
  || Boolean(modelMappingTabRef.value?.dialogOpen),
)

const deleteKeyConfirmDescription = computed(() => {
  const name = keyToDelete.value?.api_key_masked || keyToDelete.value?.name || ''
  return locale.value === 'en-US' ? `Delete key ${name}?` : `确定要删除密钥 ${name} 吗？`
})

function localizedApiError(error: unknown, fallback: string): string {
  return legacyT(parseApiError(error, fallback))
}

function handleBackdropClick() {
  if (!hasBlockingDialogOpen.value) handleClose()
}

function handleClose() {
  if (!hasBlockingDialogOpen.value) emit('update:open', false)
}

function handleEditEndpoint() {
  endpointDialogOpen.value = true
}

function handleAddKey() {
  currentEndpoint.value = endpoints.value[0] || null
  editingKey.value = null
  keyFormDialogOpen.value = true
}

function handleEditKey(key: EndpointAPIKey) {
  currentEndpoint.value = endpoints.value[0] || null
  editingKey.value = key
  keyFormDialogOpen.value = true
}

function handleKeyPermissions(key: EndpointAPIKey) {
  editingKey.value = key
  keyPermissionsDialogOpen.value = true
}

function handleDeleteKey(key: EndpointAPIKey) {
  keyToDelete.value = key
  deleteKeyConfirmOpen.value = true
}

function handleEditModel(model: Model) {
  editingModel.value = model
  modelFormDialogOpen.value = true
}

function getHealthScoreColor(score: number): string {
  if (score >= 0.8) return 'text-green-600'
  if (score >= 0.5) return 'text-amber-600'
  return 'text-destructive'
}

function getHealthScoreBarColor(score: number): string {
  if (score >= 0.8) return 'bg-green-500'
  if (score >= 0.5) return 'bg-amber-500'
  return 'bg-destructive'
}

function isKeyRecoverable(key: EndpointAPIKey): boolean {
  return Boolean(key.circuit_breaker_open || (key.consecutive_failures || 0) > 0 || (key.health_score ?? 1) < 1)
}

async function copyFullKey(key: EndpointAPIKey) {
  const cached = revealedKeys.get(key.id)
  if (cached) {
    copyToClipboard(cached)
    return
  }
  try {
    const result = await revealEndpointKey(key.id)
    const secret = result.api_key || ''
    revealedKeys.set(key.id, secret)
    copyToClipboard(secret)
  } catch (error: unknown) {
    showError(localizedApiError(error, '获取密钥失败'), legacyT('错误'))
  }
}

async function handleRecoverKey(key: EndpointAPIKey) {
  try {
    await recoverKeyHealth(key.id)
    showSuccess(legacyT('密钥健康状态已恢复'))
    await loadAllProviderData()
    emit('refresh')
  } catch (error: unknown) {
    showError(localizedApiError(error, '恢复密钥失败'), legacyT('错误'))
  }
}

async function toggleKeyActive(key: EndpointAPIKey) {
  if (togglingKeyId.value) return
  togglingKeyId.value = key.id
  try {
    const updated = await updateProviderKey(key.id, { is_active: !key.is_active })
    Object.assign(key, updated)
    emit('refresh')
  } catch (error: unknown) {
    showError(localizedApiError(error, '操作失败'), legacyT('错误'))
  } finally {
    togglingKeyId.value = null
  }
}

async function confirmDeleteKey() {
  const key = keyToDelete.value
  deleteKeyConfirmOpen.value = false
  keyToDelete.value = null
  if (!key) return
  try {
    await deleteEndpointKey(key.id)
    showSuccess(legacyT('密钥已删除'))
    await handleDataChanged()
  } catch (error: unknown) {
    showError(localizedApiError(error, '删除密钥失败'), legacyT('错误'))
  }
}

async function loadProvider() {
  if (!props.providerId) return
  try {
    provider.value = await getProvider(props.providerId)
  } catch (error: unknown) {
    showError(localizedApiError(error, '加载提供商失败'), legacyT('错误'))
  }
}

async function loadProviderKeysPage(page = currentKeyPage.value) {
  if (!props.providerId) return
  loadingProviderKeys.value = true
  try {
    const result = await getProviderKeysPage(props.providerId, { page, page_size: keyPageSize })
    providerKeys.value = result.keys
    providerKeysTotal.value = result.total
    currentKeyPage.value = result.page
  } catch (error: unknown) {
    providerKeys.value = []
    providerKeysTotal.value = 0
    showError(localizedApiError(error, '加载密钥失败'), legacyT('错误'))
  } finally {
    loadingProviderKeys.value = false
  }
}

async function loadEndpoints() {
  if (!props.providerId) return
  loadingProviderEndpoints.value = true
  try {
    const items = await getProviderEndpoints(props.providerId)
    endpoints.value = [...items].sort((a, b) => {
      const left = API_FORMAT_ORDER.indexOf(a.api_format)
      const right = API_FORMAT_ORDER.indexOf(b.api_format)
      return (left < 0 ? Number.MAX_SAFE_INTEGER : left) - (right < 0 ? Number.MAX_SAFE_INTEGER : right)
    })
  } catch (error: unknown) {
    endpoints.value = []
    showError(localizedApiError(error, '加载端点失败'), legacyT('错误'))
  } finally {
    loadingProviderEndpoints.value = false
  }
}

async function loadModels() {
  if (!props.providerId) return
  loadingProviderModels.value = true
  try {
    providerModels.value = await getProviderModels(props.providerId)
  } finally {
    loadingProviderModels.value = false
  }
}

async function loadMappingPreview() {
  if (!props.providerId) return
  loadingProviderMappingPreview.value = true
  try {
    providerMappingPreview.value = await getProviderMappingPreview(props.providerId)
  } catch {
    providerMappingPreview.value = null
  } finally {
    loadingProviderMappingPreview.value = false
  }
}

async function loadAllProviderData() {
  await Promise.all([loadProvider(), loadEndpoints(), loadProviderKeysPage(), loadModels(), loadMappingPreview()])
}

async function handleDataChanged() {
  await loadAllProviderData()
  emit('refresh')
}

async function handleModelSaved() {
  editingModel.value = null
  await handleDataChanged()
}

watch(
  [() => props.providerId, () => props.open],
  async ([providerId, open]) => {
    if (!open || !providerId) {
      revealedKeys.clear()
      return
    }
    loading.value = true
    currentKeyPage.value = 1
    if (props.initialProvider?.id === providerId) provider.value = props.initialProvider
    await loadAllProviderData()
    loading.value = false
  },
  { immediate: true },
)

useEscapeKey(() => {
  if (props.open) handleClose()
}, { disableOnInput: true, once: false })
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
