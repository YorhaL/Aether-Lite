<template>
  <div class="space-y-4 border-t border-border/60 pt-5">
    <div class="border-b border-border/60 pb-2">
      <div class="text-sm font-medium">
        {{ legacyT('流控策略') }}
      </div>
      <p class="mt-1 text-xs text-muted-foreground">
        {{ description }}
      </p>
    </div>

    <div class="space-y-4">
      <div
        v-for="field in USER_GROUP_ADMISSION_POLICY_FIELDS"
        :key="field.valueKey"
        class="space-y-2"
      >
        <Label
          :for="`user-group-admission-${field.valueKey}`"
          class="text-sm font-medium"
        >
          {{ legacyT(field.label) }}（{{ legacyT(field.unit) }}）
        </Label>
        <div class="flex flex-col gap-2 sm:flex-row sm:items-center">
          <div class="flex min-h-10 w-full items-center gap-2 sm:w-auto sm:shrink-0">
            <Switch
              :model-value="form[field.modeKey] === 'system'"
              @update:model-value="setSystemMode(field.modeKey, $event)"
            />
            <span class="text-xs text-muted-foreground sm:sr-only">
              {{ legacyT(form[field.modeKey] === 'system' ? '系统默认' : '自定义') }}
            </span>
          </div>
          <div class="min-w-0 flex-1">
            <Input
              :id="`user-group-admission-${field.valueKey}`"
              :model-value="form[field.valueKey] ?? ''"
              type="number"
              min="0"
              :max="field.max"
              :step="field.step"
              class="h-10"
              :disabled="form[field.modeKey] === 'system'"
              :placeholder="legacyT(form[field.modeKey] === 'system' ? '使用系统默认' : '0 = 不限制')"
              @update:model-value="updatePolicyValue(field, $event)"
            />
          </div>
        </div>
        <p class="text-xs text-muted-foreground">
          {{ legacyT('0 表示不限制') }}
          <span v-if="form[field.modeKey] === 'system'">
            · {{ legacyT('当前跟随系统设置') }}
          </span>
        </p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { Input, Label, Switch } from '@/components/ui'
import { useI18n } from '@/i18n'
import { parseNumberInput } from '@/utils/form'
import type { UserGroupFormState } from './user-management-types'
import {
  USER_GROUP_ADMISSION_POLICY_FIELDS,
  type UserGroupAdmissionModeKey,
  type UserGroupAdmissionPolicyField,
} from './userGroupAdmissionPolicy'

const props = defineProps<{
  form: UserGroupFormState
}>()

const emit = defineEmits<{
  'update:form': [value: UserGroupFormState]
}>()

const { legacyT, locale } = useI18n()
const description = computed(() => locale.value === 'en-US'
  ? 'Use the system default or set a group value. Multiple groups grant the highest value; 0 means unlimited.'
  : '可跟随系统默认或单独设置；多个用户组取更高额度，0 表示不限制。')

function updateForm(patch: Partial<UserGroupFormState>): void {
  emit('update:form', { ...props.form, ...patch })
}

function setSystemMode(modeKey: UserGroupAdmissionModeKey, system: boolean): void {
  updateForm({ [modeKey]: system ? 'system' : 'custom' })
}

function updatePolicyValue(field: UserGroupAdmissionPolicyField, value: string | number): void {
  updateForm({
    [field.valueKey]: parseNumberInput(value, {
      allowFloat: !field.integer,
      min: 0,
      max: field.max,
    }),
  })
}
</script>
