<template>
  <TableCard :title="title">
    <template #actions>
      <slot name="actions">
        <Select
          v-if="showMetricSelect"
          :model-value="metric"
          @update:model-value="emitMetric"
        >
          <SelectTrigger class="h-8 text-xs w-28">
            <SelectValue placeholder="指标" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="requests">
              请求数
            </SelectItem>
            <SelectItem value="tokens">
              Tokens
            </SelectItem>
            <SelectItem value="cost">
              成本
            </SelectItem>
          </SelectContent>
        </Select>
      </slot>
    </template>

    <div
      v-if="loading"
      class="p-6"
    >
      <LoadingState />
    </div>
    <div
      v-else-if="items.length === 0"
      class="p-6"
    >
      <EmptyState
        title="暂无数据"
        description="当前时间范围内没有统计结果"
      />
    </div>
    <Table v-else>
      <TableHeader>
        <TableRow>
          <TableHead class="w-16">
            排名
          </TableHead>
          <TableHead>名称</TableHead>
          <TableHead class="text-right">
            请求数
          </TableHead>
          <TableHead class="text-right">
            Tokens
          </TableHead>
          <TableHead class="text-right">
            成本
          </TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <TableRow
          v-for="item in items"
          :key="item.id"
          :class="selectable ? 'cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring' : undefined"
          :data-state="selectable && selectedItemId === item.id ? 'selected' : undefined"
          :tabindex="selectable ? 0 : undefined"
          :aria-selected="selectable ? selectedItemId === item.id : undefined"
          @click="selectItem(item)"
          @keydown.enter.prevent="selectItem(item)"
          @keydown.space.prevent="selectItem(item)"
        >
          <TableCell class="font-medium">
            {{ item.rank }}
          </TableCell>
          <TableCell>
            <slot
              name="name"
              :item="item"
            >
              {{ item.name }}
            </slot>
          </TableCell>
          <TableCell class="text-right">
            {{ item.requests }}
          </TableCell>
          <TableCell class="text-right">
            {{ formatTokens(item.tokens) }}
          </TableCell>
          <TableCell class="text-right">
            {{ formatCurrency(item.cost) }}
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>

    <slot name="pagination" />
  </TableCard>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { EmptyState, LoadingState } from '@/components/common'
import { TableCard } from '@/components/ui'
import {
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
  TableRow
} from '@/components/ui'
import { formatCurrency, formatTokens } from '@/utils/format'
import type { LeaderboardItem } from '@/api/admin'

interface Props {
  title: string
  items: LeaderboardItem[]
  metric: 'requests' | 'tokens' | 'cost'
  loading?: boolean
  showMetricSelect?: boolean
  selectable?: boolean
  selectedItemId?: string | null
}

const props = withDefaults(defineProps<Props>(), {
  loading: false,
  showMetricSelect: true,
  selectable: false,
  selectedItemId: null,
})

const emit = defineEmits<{
  'update:metric': [value: 'requests' | 'tokens' | 'cost']
  'select-item': [item: LeaderboardItem]
}>()

const metric = computed(() => props.metric)

function emitMetric(value: string) {
  if (value === 'requests' || value === 'tokens' || value === 'cost') {
    emit('update:metric', value)
  }
}

function selectItem(item: LeaderboardItem) {
  if (props.selectable) {
    emit('select-item', item)
  }
}
</script>
