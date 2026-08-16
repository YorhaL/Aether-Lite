<template>
  <CardSection
    title="流控策略"
    description="统一配置系统范围的请求准入限制"
  >
    <template #actions>
      <Button
        size="sm"
        :disabled="loading || !hasChanges"
        @click="$emit('save')"
      >
        {{ loading ? '保存中...' : '保存' }}
      </Button>
    </template>

    <div class="grid grid-cols-1 gap-6 md:grid-cols-3">
      <div
        v-for="field in SYSTEM_ADMISSION_POLICY_FIELDS"
        :key="field.key"
      >
        <Label
          :for="`system-admission-${field.key}`"
          class="block text-sm font-medium"
        >
          {{ field.label }}（{{ field.unit }}）
        </Label>
        <Input
          :id="`system-admission-${field.key}`"
          :model-value="config[field.key]"
          type="number"
          min="0"
          :max="field.max"
          :step="field.step"
          placeholder="0"
          class="mt-1"
          @update:model-value="updateField(field, $event)"
        />
        <p class="mt-1 text-xs text-muted-foreground">
          0 表示不限制；未单独配置的用户和独立 Key 会跟随这里
        </p>
      </div>
    </div>
  </CardSection>
</template>

<script setup lang="ts">
import { CardSection } from '@/components/layout'
import Button from '@/components/ui/button.vue'
import Input from '@/components/ui/input.vue'
import Label from '@/components/ui/label.vue'
import {
  normalizeSystemAdmissionPolicyValue,
  SYSTEM_ADMISSION_POLICY_FIELDS,
  type SystemAdmissionPolicyConfig,
  type SystemAdmissionPolicyConfigKey,
  type SystemAdmissionPolicyField,
} from './admissionPolicyConfig'

defineProps<{
  config: SystemAdmissionPolicyConfig
  loading: boolean
  hasChanges: boolean
}>()

const emit = defineEmits<{
  save: []
  'update:configValue': [key: SystemAdmissionPolicyConfigKey, value: number]
}>()

function updateField(field: SystemAdmissionPolicyField, rawValue: unknown) {
  emit('update:configValue', field.key, normalizeSystemAdmissionPolicyValue(field, rawValue))
}
</script>
