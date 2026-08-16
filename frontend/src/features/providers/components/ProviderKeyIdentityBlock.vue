<template>
  <div class="flex flex-col min-w-0">
    <div class="flex items-center gap-1.5">
      <span
        class="text-sm font-medium truncate"
        :class="apiKey.name ? 'cursor-pointer hover:text-primary transition-colors' : ''"
        :title="apiKey.name ? legacyT('点击复制') : ''"
        data-testid="provider-key-name"
        @click.stop="apiKey.name && $emit('copyName', apiKey.name)"
      >
        {{ apiKey.name || legacyT('未命名密钥') }}
      </span>
    </div>

    <div class="flex items-center gap-1">
      <span class="text-[11px] font-mono text-muted-foreground">
        {{ maskedSecretLabel }}
      </span>

      <Button
        variant="ghost"
        size="icon"
        class="h-4 w-4 shrink-0"
        :title="legacyT('复制密钥')"
        @click.stop="$emit('copyFullKey')"
      >
        <Copy class="w-2.5 h-2.5" />
      </Button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Copy } from 'lucide-vue-next'
import Button from '@/components/ui/button.vue'
import { useI18n } from '@/i18n'
import type { EndpointAPIKey } from '@/api/endpoints'

defineProps<{
  apiKey: EndpointAPIKey
  maskedSecretLabel: string
}>()

defineEmits<{
  (e: 'copyName', name: string): void
  (e: 'copyFullKey'): void
}>()

const { legacyT } = useI18n()

</script>
