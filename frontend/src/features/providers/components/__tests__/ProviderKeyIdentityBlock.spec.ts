import { describe, expect, it, vi } from 'vitest'
import { createApp, defineComponent, h } from 'vue'

import ProviderKeyIdentityBlock from '@/features/providers/components/ProviderKeyIdentityBlock.vue'
import type { EndpointAPIKey } from '@/api/endpoints'
import { createI18n } from '@/i18n'

vi.mock('lucide-vue-next', async () => {
  const { defineComponent, h } = await import('vue')
  const Icon = defineComponent({
    name: 'IconStub',
    setup() {
      return () => h('span')
    },
  })

  return {
    Copy: Icon,
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
    health_score: 1,
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
      return () => h(ProviderKeyIdentityBlock, props)
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

describe('ProviderKeyIdentityBlock', () => {
  it('renders a custom key identity and emits copy actions', () => {
    const onCopyName = vi.fn()
    const onCopyFullKey = vi.fn()

    const { root, unmount } = mount({
      apiKey: createProviderKey(),
      maskedSecretLabel: 'sk-***',
      onCopyName,
      onCopyFullKey,
    })

    expect(root.querySelector('[data-testid="provider-key-name"]')?.textContent).toContain('Primary key')
    expect(root.textContent).toContain('sk-***')

    ;(root.querySelector('[data-testid="provider-key-name"]') as HTMLElement).click()
    ;(root.querySelector('button[title="复制密钥"]') as HTMLButtonElement).click()

    expect(onCopyName).toHaveBeenCalledWith('Primary key')
    expect(onCopyFullKey).toHaveBeenCalledTimes(1)

    unmount()
  })

  it('renders an unnamed key', () => {
    const onCopyFullKey = vi.fn()

    const { root, unmount } = mount({
      apiKey: createProviderKey({ name: '' }),
      maskedSecretLabel: 'sk-***',
      onCopyFullKey,
    })

    expect(root.querySelector('[data-testid="provider-key-name"]')?.textContent).toContain('未命名密钥')

    ;(root.querySelector('button[title="复制密钥"]') as HTMLButtonElement).click()

    expect(onCopyFullKey).toHaveBeenCalledTimes(1)

    unmount()
  })
})
