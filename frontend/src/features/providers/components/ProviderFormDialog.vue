<template>
  <Dialog
    :model-value="internalOpen"
    :title="legacyT(isEditMode ? '编辑提供商' : '添加提供商')"
    :description="legacyT(isEditMode ? '更新提供商配置。API 端点和密钥需在详情页面单独管理。' : '创建新的提供商配置。创建后可以为其添加 API 端点和密钥。')"
    :icon="isEditMode ? SquarePen : Server"
    size="xl"
    @update:model-value="handleDialogUpdate"
  >
    <form
      class="space-y-5"
      @submit.prevent="handleSubmit"
    >
      <!-- 基本信息 -->
      <div class="space-y-3">
        <h3 class="text-sm font-medium border-b pb-2">
          {{ legacyT('基本信息') }}
        </h3>

        <div class="space-y-1.5">
          <Label for="name">{{ legacyT('名称 *') }}</Label>
          <Input
            id="name"
            v-model="form.name"
            :placeholder="legacyT('例如: OpenAI 主账号')"
          />
        </div>

        <div class="space-y-1.5">
          <Label for="website">{{ legacyT('主站链接') }}</Label>
          <Input
            id="website"
            v-model="form.website"
            :placeholder="legacyT('https://example.com（可选）')"
          />
        </div>
      </div>

      <!-- 请求配置 -->
      <div class="space-y-3">
        <h3 class="text-sm font-medium border-b pb-2">
          {{ legacyT('请求配置') }}
        </h3>
        <div class="grid grid-cols-2 gap-4">
          <div class="space-y-1.5">
            <Label>{{ legacyT('最大重试次数') }}</Label>
            <Input
              :model-value="form.max_retries ?? ''"
              type="number"
              min="0"
              max="999"
              :placeholder="legacyT('默认 2')"
              @update:model-value="(v) => form.max_retries = parseNumberInput(v)"
            />
          </div>
        </div>

        <!-- 超时配置 -->
        <div class="grid grid-cols-2 gap-4">
          <div class="space-y-1.5">
            <Label>
              {{ legacyT('流式首字节超时') }}
              <span class="text-xs text-muted-foreground">{{ legacyT('(秒)') }}</span>
            </Label>
            <Input
              :model-value="form.stream_first_byte_timeout ?? ''"
              type="number"
              min="1"
              max="300"
              step="1"
              placeholder="30"
              @update:model-value="(v) => form.stream_first_byte_timeout = parseNumberInput(v)"
            />
          </div>
          <div class="space-y-1.5">
            <Label>
              {{ legacyT('非流式请求超时') }}
              <span class="text-xs text-muted-foreground">{{ legacyT('(秒)') }}</span>
            </Label>
            <Input
              :model-value="form.request_timeout ?? ''"
              type="number"
              min="1"
              max="1200"
              step="1"
              placeholder="300"
              @update:model-value="(v) => form.request_timeout = parseNumberInput(v)"
            />
          </div>
        </div>

        <div class="flex items-center justify-between gap-4 rounded-lg border p-3">
          <div class="space-y-1">
            <Label for="responses-websocket-enabled">
              {{ legacyT('Responses WebSocket') }}
            </Label>
            <p class="text-xs text-muted-foreground">
              {{ legacyT('仅当该提供商的 OpenAI Responses 端点支持标准 WebSocket Upgrade 时开启。') }}
            </p>
          </div>
          <Switch
            id="responses-websocket-enabled"
            v-model="form.responses_websocket_enabled"
          />
        </div>
      </div>
    </form>

    <template #footer>
      <Button
        type="button"
        variant="outline"
        :disabled="loading"
        @click="handleCancel"
      >
        {{ legacyT('取消') }}
      </Button>
      <Button
        :disabled="loading || !form.name"
        @click="handleSubmit"
      >
        {{ submitLabel }}
      </Button>
    </template>
  </Dialog>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import {
  Dialog,
  Button,
  Input,
  Label,
  Switch,
} from '@/components/ui'
import { Server, SquarePen } from 'lucide-vue-next'
import { useToast } from '@/composables/useToast'
import { useFormDialog } from '@/composables/useFormDialog'
import { useI18n } from '@/i18n'
import {
  createProvider,
  updateProvider,
  type ProviderWithEndpointsSummary,
} from '@/api/endpoints'
import { parseApiError } from '@/utils/errorParser'
import { parseNumberInput } from '@/utils/form'

const props = defineProps<{
  modelValue: boolean
  provider?: ProviderWithEndpointsSummary | null  // 编辑模式时传入
  maxPriority?: number  // 当前已有的最大优先级值
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  'providerCreated': []
  'providerUpdated': [provider: ProviderWithEndpointsSummary]
}>()

const { success, error: showError } = useToast()
const { legacyT } = useI18n()
const loading = ref(false)

// 内部状态
const internalOpen = computed(() => props.modelValue)

// 计算新建时的默认优先级
const defaultPriority = computed(() => {
  if (props.maxPriority != null) {
    return Math.min(props.maxPriority + 10, 10000)
  }
  return 100
})

const submitLabel = computed(() => {
  if (loading.value) {
    return legacyT(isEditMode.value ? '保存中...' : '创建中...')
  }
  return legacyT(isEditMode.value ? '保存' : '创建')
})

// 表单数据
const form = ref({
  name: '',
  description: '',
  website: '',
  provider_priority: 100,
  // 状态配置
  is_active: true,
  rate_limit: undefined as number | undefined,
  concurrent_limit: undefined as number | undefined,
  // 请求配置
  max_retries: undefined as number | undefined,
  // 超时配置（秒）
  stream_first_byte_timeout: undefined as number | undefined,
  request_timeout: undefined as number | undefined,
  responses_websocket_enabled: false,
})

// 重置表单
function resetForm() {
  form.value = {
    name: '',
    description: '',
    website: '',
    provider_priority: defaultPriority.value,
    is_active: true,
    rate_limit: undefined,
    concurrent_limit: undefined,
    // 请求配置
    max_retries: undefined,
    // 超时配置
    stream_first_byte_timeout: undefined,
    request_timeout: undefined,
    responses_websocket_enabled: false,
  }
}

// 加载提供商数据（编辑模式）
function loadProviderData() {
  if (!props.provider) return
  form.value = {
    name: props.provider.name,
    description: props.provider.description || '',
    website: props.provider.website || '',
    provider_priority: props.provider.provider_priority || 999,
    is_active: props.provider.is_active,
    rate_limit: undefined,
    concurrent_limit: undefined,
    // 请求配置
    max_retries: props.provider.max_retries ?? undefined,
    // 超时配置
    stream_first_byte_timeout: props.provider.stream_first_byte_timeout ?? undefined,
    request_timeout: props.provider.request_timeout ?? undefined,
    responses_websocket_enabled: props.provider.responses_websocket_enabled === true,
  }
}

// 使用 useFormDialog 统一处理对话框逻辑
const { isEditMode, handleDialogUpdate, handleCancel } = useFormDialog({
  isOpen: () => props.modelValue,
  entity: () => props.provider,
  isLoading: loading,
  onClose: () => emit('update:modelValue', false),
  loadData: loadProviderData,
  resetForm,
})

// 提交表单
const handleSubmit = async () => {
  loading.value = true
  try {
    const basePayload = {
      name: form.value.name,
      description: form.value.description || undefined,
      website: form.value.website || undefined,
      is_active: form.value.is_active,
      // 请求配置
      max_retries: form.value.max_retries ?? undefined,
      // 超时配置（null 表示清除，使用全局配置）
      stream_first_byte_timeout: form.value.stream_first_byte_timeout ?? null,
      request_timeout: form.value.request_timeout ?? null,
      responses_websocket_enabled: form.value.responses_websocket_enabled,
    }

    if (isEditMode.value && props.provider) {
      // 更新提供商
      const updated = await updateProvider(props.provider.id, {
        ...basePayload,
        provider_priority: form.value.provider_priority,
      })
      success(legacyT('提供商更新成功'))
      emit('providerUpdated', updated)
    } else {
      // 创建提供商（优先级由后端自动置顶）
      await createProvider(basePayload)
      success(legacyT('提供商已创建，请继续添加端点和密钥，或在优先级管理中调整顺序'), legacyT('创建成功'))
      emit('providerCreated')
    }

    emit('update:modelValue', false)
  } catch (error: unknown) {
    const action = isEditMode.value ? '更新' : '创建'
    showError(parseApiError(error, legacyT(`${action}提供商失败`)), legacyT(`${action}失败`))
  } finally {
    loading.value = false
  }
}
</script>
