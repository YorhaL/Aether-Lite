<template>
  <div class="rounded-2xl border border-border/60 bg-card/95 p-4 shadow-[0_10px_26px_-22px_hsl(var(--foreground))]">
    <div class="space-y-4">
      <div class="flex items-start gap-3">
        <Checkbox
          class="mt-2 shrink-0"
          :checked="selected"
          :disabled="selectionDisabled"
          @update:checked="(checked) => $emit('toggle-selected', checked === true)"
        />
        <UserIdentityCell
          :row="row"
          :show-groups="false"
        />
      </div>

      <UserStatusBadges
        :row="row"
        mobile
      />

      <UserWalletSummary
        :row="row"
        mobile
      />

      <section class="space-y-2">
        <h3 class="text-xs font-semibold text-foreground">
          {{ legacyT('统计') }}
        </h3>
        <div class="grid grid-cols-2 gap-2.5 text-xs">
          <div class="rounded-lg border border-border/50 bg-background/70 p-2.5">
            <div class="mb-1 text-muted-foreground">
              {{ legacyT('请求次数') }}
            </div>
            <div class="font-semibold text-foreground">
              {{ row.requestCountLabel }}
            </div>
          </div>
          <div class="rounded-lg border border-border/50 bg-background/70 p-2.5">
            <div class="mb-1 text-muted-foreground">
              Tokens
            </div>
            <div class="font-semibold text-foreground">
              {{ row.tokensLabel }}
            </div>
          </div>
        </div>
      </section>

      <section class="space-y-2">
        <h3 class="text-xs font-semibold text-foreground">
          {{ legacyT('流控策略') }}
        </h3>
        <div class="space-y-2 rounded-lg border border-border/50 bg-background/70 p-2.5 text-xs">
          <div
            class="flex items-center justify-between gap-3"
            :title="legacyT(row.rateLimitSource)"
          >
            <span class="text-muted-foreground">RPM:</span>
            <div class="min-w-0 text-right">
              <Badge
                v-if="row.rateLimitAsBadge"
                variant="secondary"
                class="h-5 px-1.5 py-0 text-[10px] font-medium"
              >
                {{ legacyT(row.rateLimitLabel) }}
              </Badge>
              <div
                v-else
                class="font-medium text-foreground"
              >
                {{ legacyT(row.rateLimitLabel) }}
              </div>
            </div>
          </div>
          <div
            class="flex items-center justify-between gap-3"
            :title="legacyT(row.dailyUsageLimitSource)"
          >
            <span class="text-muted-foreground">{{ legacyT('日限额:') }}</span>
            <div class="min-w-0 text-right">
              <Badge
                v-if="row.dailyUsageLimitAsBadge"
                variant="secondary"
                class="h-5 px-1.5 py-0 text-[10px] font-medium"
              >
                {{ legacyT(row.dailyUsageLimitLabel) }}
              </Badge>
              <div
                v-else
                class="font-medium text-foreground"
              >
                {{ legacyT(row.dailyUsageLimitLabel) }}
              </div>
            </div>
          </div>
          <div
            class="flex items-center justify-between gap-3"
            :title="legacyT(row.concurrentLimitSource)"
          >
            <span class="text-muted-foreground">{{ legacyT('并发') }}:</span>
            <div class="min-w-0 text-right">
              <Badge
                v-if="row.concurrentLimitAsBadge"
                variant="secondary"
                class="h-5 px-1.5 py-0 text-[10px] font-medium"
              >
                {{ legacyT(row.concurrentLimitLabel) }}
              </Badge>
              <div
                v-else
                class="font-medium text-foreground"
              >
                {{ legacyT(row.concurrentLimitLabel) }}
              </div>
            </div>
          </div>
        </div>
      </section>

      <div class="rounded-lg bg-muted/35 p-2.5 text-[11px] text-muted-foreground">
        <div class="flex items-center justify-between gap-2">
          <span>{{ legacyT('创建时间') }}</span>
          <span class="font-medium text-foreground">{{ row.createdAtLabel }}</span>
        </div>
      </div>

      <UserActionButtons
        :can-operate-admin="canOperateAdmin"
        :is-active="row.user.is_active"
        mobile
        @edit="$emit('edit')"
        @wallet="$emit('wallet')"
        @api-keys="$emit('api-keys')"
        @sessions="$emit('sessions')"
        @toggle-status="$emit('toggle-status')"
        @delete="$emit('delete')"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import Badge from '@/components/ui/badge.vue'
import Checkbox from '@/components/ui/checkbox.vue'
import { useI18n } from '@/i18n'
import UserActionButtons from './UserActionButtons.vue'
import UserIdentityCell from './UserIdentityCell.vue'
import UserStatusBadges from './UserStatusBadges.vue'
import UserWalletSummary from './UserWalletSummary.vue'
import type { UserManagementRow } from './user-management-types'

defineProps<{
  row: UserManagementRow
  selected: boolean
  selectionDisabled: boolean
  canOperateAdmin: boolean
}>()

defineEmits<{
  'toggle-selected': [checked: boolean]
  edit: []
  wallet: []
  'api-keys': []
  sessions: []
  'toggle-status': []
  delete: []
}>()

const { legacyT } = useI18n()
</script>
