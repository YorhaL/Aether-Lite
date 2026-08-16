import { describe, expect, it, vi } from 'vitest'
import { createApp, defineComponent, h } from 'vue'

import ProviderKeyActionCluster from '@/features/providers/components/ProviderKeyActionCluster.vue'
import type { EndpointAPIKey } from '@/api/endpoints'
import { createI18n } from '@/i18n'

vi.mock('@/components/ui', async () => {
  const { defineComponent, h } = await import('vue')

  const passthrough = (name: string) => defineComponent({
    name,
    setup(_, { slots }) {
      return () => h('div', slots.default?.())
    },
  })

  return {
    Popover: passthrough('PopoverStub'),
    PopoverTrigger: passthrough('PopoverTriggerStub'),
    PopoverContent: passthrough('PopoverContentStub'),
  }
})

vi.mock('lucide-vue-next', async () => {
  const { defineComponent, h } = await import('vue')
  const Icon = defineComponent({
    name: 'IconStub',
    setup() {
      return () => h('span')
    },
  })

  return {
    Edit: Icon,
    Power: Icon,
    RefreshCw: Icon,
    Shield: Icon,
    Trash2: Icon,
  }
})

function createProviderKey(overrides: Partial<EndpointAPIKey> = {}): EndpointAPIKey {
  return {
    id: 'provider-key-1',
    provider_id: 'provider-1',
    api_formats: ['openai:chat'],
    api_key_masked: 'sk-***',
    auth_type: 'api_key',
    name: 'Primary key',
    internal_priority: 10,
    cache_ttl_minutes: 0,
    max_probe_interval_minutes: 5,
    health_score: 0.42,
    consecutive_failures: 0,
    request_count: 0,
    success_count: 0,
    error_count: 0,
    success_rate: 1,
    avg_response_time_ms: 0,
    is_active: true,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  }
}

function mount(props: Record<string, unknown>) {
  const root = document.createElement('div')
  document.body.appendChild(root)
  const app = createApp(defineComponent({
    setup() {
      return () => h(ProviderKeyActionCluster, props)
    },
  }))
  app.use(createI18n())
  app.mount(root)

  return {
    root,
    unmount: () => {
      app.unmount()
      root.remove()
    },
  }
}

describe('ProviderKeyActionCluster', () => {
  it('renders circuit, health and key actions', () => {
    const { root, unmount } = mount({
      apiKey: createProviderKey({
        circuit_breaker_open: true,
      }),
      recoverable: true,
      recoverTitle: 'Recover key',
      circuitBreakerTitle: 'Circuit is open',
      circuitProbeCountdown: ' 2m',
      healthScoreBarClass: 'bg-red-500',
      healthScoreTextClass: 'text-red-600',
    })

    expect(root.querySelector('[data-testid="provider-key-circuit-badge"]')?.textContent).toContain('熔断 2m')
    expect(root.querySelector('[data-testid="provider-key-health"]')?.textContent).toContain('42%')
    expect(root.querySelector('button[title="Recover key"]')).toBeTruthy()
    expect(root.querySelector('[data-testid="provider-key-toggle-active"]')?.getAttribute('aria-label')).toBe('点击停用')

    unmount()
  })

  it('emits key operation events without owning business logic', () => {
    const onRecover = vi.fn()
    const onPermissions = vi.fn()
    const onEdit = vi.fn()
    const onToggleActive = vi.fn()
    const onDelete = vi.fn()

    const { root, unmount } = mount({
      apiKey: createProviderKey(),
      recoverable: true,
      recoverTitle: 'Recover key',
      onRecover,
      onPermissions,
      onEdit,
      onToggleActive,
      onDelete,
    })

    ;(root.querySelector('button[title="Recover key"]') as HTMLButtonElement).click()
    ;(root.querySelector('button[title="模型权限"]') as HTMLButtonElement).click()
    ;(root.querySelector('button[title="编辑密钥"]') as HTMLButtonElement).click()
    ;(root.querySelector('button[title="点击停用"]') as HTMLButtonElement).click()
    ;(root.querySelector('button[title="删除密钥"]') as HTMLButtonElement).click()

    expect(onRecover).toHaveBeenCalledTimes(1)
    expect(onPermissions).toHaveBeenCalledTimes(1)
    expect(onEdit).toHaveBeenCalledTimes(1)
    expect(onToggleActive).toHaveBeenCalledTimes(1)
    expect(onDelete).toHaveBeenCalledTimes(1)

    unmount()
  })

  it('offers an enable action for an inactive account', () => {
    const onToggleActive = vi.fn()
    const { root, unmount } = mount({
      apiKey: createProviderKey({ is_active: false }),
      onToggleActive,
    })

    const enableButton = root.querySelector('[data-testid="provider-key-toggle-active"]') as HTMLButtonElement | null
    expect(enableButton).toBeTruthy()
    expect(enableButton?.getAttribute('title')).toBe('点击启用')
    expect(enableButton?.getAttribute('aria-label')).toBe('点击启用')

    enableButton?.click()
    expect(onToggleActive).toHaveBeenCalledTimes(1)

    unmount()
  })
})
