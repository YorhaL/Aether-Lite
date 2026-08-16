<template>
  <div class="flex items-center gap-1 shrink-0">
    <Badge
      v-if="apiKey.circuit_breaker_open"
      variant="destructive"
      class="text-[10px] px-1.5 py-0 shrink-0"
      :title="circuitBreakerTitle"
      data-testid="provider-key-circuit-badge"
    >
      {{ legacyT('熔断') }}{{ circuitProbeCountdown }}
    </Badge>

    <div
      v-if="apiKey.health_score !== undefined"
      class="flex items-center gap-1 mr-1"
      data-testid="provider-key-health"
    >
      <div class="w-10 h-1.5 bg-border rounded-full overflow-hidden">
        <div
          class="h-full transition-all duration-300"
          :class="healthScoreBarClass"
          :style="{ width: `${healthScorePercent}%` }"
        />
      </div>
      <span
        class="text-[10px] font-medium tabular-nums"
        :class="healthScoreTextClass"
      >
        {{ healthScorePercent.toFixed(0) }}%
      </span>
    </div>

    <Button
      v-if="recoverable"
      variant="ghost"
      size="icon"
      class="h-7 w-7 text-green-600"
      :title="recoverTitle"
      @click="$emit('recover')"
    >
      <RefreshCw class="w-3.5 h-3.5" />
    </Button>

    <Button
      variant="ghost"
      size="icon"
      class="h-7 w-7"
      :title="legacyT('模型权限')"
      @click="$emit('permissions')"
    >
      <Shield class="w-3.5 h-3.5" />
    </Button>

    <Button
      variant="ghost"
      size="icon"
      class="h-7 w-7"
      :title="legacyT('编辑密钥')"
      @click="$emit('edit')"
    >
      <Edit class="w-3.5 h-3.5" />
    </Button>

    <Button
      variant="ghost"
      size="icon"
      class="h-7 w-7"
      :disabled="toggling"
      :title="legacyT(apiKey.is_active ? '点击停用' : '点击启用')"
      :aria-label="legacyT(apiKey.is_active ? '点击停用' : '点击启用')"
      data-testid="provider-key-toggle-active"
      @click="$emit('toggleActive')"
    >
      <Power class="w-3.5 h-3.5" />
    </Button>

    <Button
      variant="ghost"
      size="icon"
      class="h-7 w-7"
      :title="legacyT('删除密钥')"
      @click="$emit('delete')"
    >
      <Trash2 class="w-3.5 h-3.5" />
    </Button>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import {
  Edit,
  Power,
  RefreshCw,
  Shield,
  Trash2,
} from 'lucide-vue-next'
import Button from '@/components/ui/button.vue'
import Badge from '@/components/ui/badge.vue'
import { useI18n } from '@/i18n'
import type { EndpointAPIKey } from '@/api/endpoints'

const props = withDefaults(defineProps<{
  apiKey: EndpointAPIKey
  recoverable?: boolean
  recoverTitle?: string
  circuitBreakerTitle?: string
  circuitProbeCountdown?: string
  healthScoreBarClass?: string
  healthScoreTextClass?: string
  toggling?: boolean
}>(), {
  recoverable: false,
  recoverTitle: '',
  circuitBreakerTitle: '',
  circuitProbeCountdown: '',
  healthScoreBarClass: '',
  healthScoreTextClass: '',
  toggling: false,
})

defineEmits<{
  (e: 'recover'): void
  (e: 'permissions'): void
  (e: 'edit'): void
  (e: 'toggleActive'): void
  (e: 'delete'): void
}>()

const { legacyT } = useI18n()

const healthScorePercent = computed(() => {
  const score = Number(props.apiKey.health_score ?? 0)
  if (!Number.isFinite(score)) return 0
  return Math.min(Math.max(score * 100, 0), 100)
})
</script>
