import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, defineComponent, h, nextTick, type App } from 'vue'

import RoutingProfiles from '../RoutingProfiles.vue'
import type {
  RoutingGroupCreateRequest,
  RoutingGroupRecord,
  RoutingGroupUpdateRequest,
} from '@/api/routing-profiles'

const apiMocks = vi.hoisted(() => ({
  listRoutingGroups: vi.fn(),
  createRoutingGroup: vi.fn(),
  updateRoutingGroup: vi.fn(),
  deleteRoutingGroup: vi.fn(),
  getGlobalModels: vi.fn(),
}))
const routeMocks = vi.hoisted(() => ({
  route: null as null | { name: string; params: Record<string, string> },
  push: vi.fn(),
  replace: vi.fn(),
}))
const toastMocks = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }))

vi.mock('vue-router', async () => {
  const { reactive } = await import('vue')
  routeMocks.route = reactive({
    name: 'RoutingProfileDetail',
    params: { groupId: 'group-1' },
  })
  return {
    useRoute: () => routeMocks.route,
    useRouter: () => ({ push: routeMocks.push, replace: routeMocks.replace }),
  }
})

vi.mock('@/api/routing-profiles', () => ({
  listRoutingGroups: apiMocks.listRoutingGroups,
  createRoutingGroup: apiMocks.createRoutingGroup,
  updateRoutingGroup: apiMocks.updateRoutingGroup,
  deleteRoutingGroup: apiMocks.deleteRoutingGroup,
}))

vi.mock('@/api/global-models', () => ({ getGlobalModels: apiMocks.getGlobalModels }))
vi.mock('@/composables/useToast', () => ({ useToast: () => toastMocks }))
vi.mock('@/utils/logger', () => ({ log: { error: vi.fn() } }))

vi.mock('@/components/layout', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    PageContainer: defineComponent({
      setup(_, { slots }) {
        return () => h('main', slots.default?.())
      },
    }),
  }
})

vi.mock('@/components/ui', async () => {
  const { defineComponent, h } = await import('vue')

  const wrapper = (tag = 'div') => defineComponent({
    inheritAttrs: false,
    props: { class: String },
    setup(props, { attrs, slots }) {
      return () => h(tag, { ...attrs, class: props.class }, [
        slots.header?.(),
        slots.default?.(),
      ])
    },
  })

  const Input = defineComponent({
    inheritAttrs: false,
    props: {
      modelValue: { type: [String, Number], default: '' },
      class: String,
      disabled: Boolean,
    },
    emits: ['update:modelValue'],
    setup(props, { attrs, emit }) {
      return () => h('input', {
        ...attrs,
        class: props.class,
        disabled: props.disabled,
        value: props.modelValue,
        onInput: (event: Event) => emit(
          'update:modelValue',
          (event.target as HTMLInputElement).value,
        ),
      })
    },
  })

  const Textarea = defineComponent({
    inheritAttrs: false,
    props: {
      modelValue: { type: String, default: '' },
      class: String,
      disabled: Boolean,
    },
    emits: ['update:modelValue'],
    setup(props, { attrs, emit }) {
      return () => h('textarea', {
        ...attrs,
        class: props.class,
        disabled: props.disabled,
        value: props.modelValue,
        onInput: (event: Event) => emit(
          'update:modelValue',
          (event.target as HTMLTextAreaElement).value,
        ),
      })
    },
  })

  const Button = defineComponent({
    inheritAttrs: false,
    props: {
      class: String,
      disabled: Boolean,
      type: { type: String, default: 'button' },
    },
    setup(props, { attrs, slots }) {
      return () => h('button', {
        ...attrs,
        class: props.class,
        disabled: props.disabled,
        type: props.type,
      }, slots.default?.())
    },
  })

  return {
    Badge: wrapper(),
    Button,
    Card: wrapper('section'),
    Input,
    Table: wrapper('table'),
    TableBody: wrapper('tbody'),
    TableCard: wrapper(),
    TableCell: wrapper('td'),
    TableHead: wrapper('th'),
    TableHeader: wrapper('thead'),
    TableRow: wrapper('tr'),
    Textarea,
  }
})

vi.mock('@/components/ui/dropdown-menu', async () => {
  const { defineComponent, h } = await import('vue')
  const wrapper = defineComponent({
    setup(_, { slots }) {
      return () => h('div', slots.default?.())
    },
  })
  return {
    DropdownMenu: wrapper,
    DropdownMenuContent: wrapper,
    DropdownMenuItem: wrapper,
    DropdownMenuTrigger: wrapper,
  }
})

vi.mock('@/components/common', async () => {
  const { defineComponent, h } = await import('vue')
  return { AlertDialog: defineComponent({ setup: () => () => h('div') }) }
})

vi.mock('@/features/routing/components', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    RoutingPriorityPolicyEditor: defineComponent({
      setup: () => () => h('div', { 'data-testid': 'routing-policy-editor' }),
    }),
  }
})

let app: App | undefined
let root: HTMLElement | undefined

function routingGroup(
  allowedModels: string[] = [],
  overrides: Partial<RoutingGroupRecord> = {},
): RoutingGroupRecord {
  return {
    id: 'group-1',
    name: 'Default routing',
    description: null,
    enabled: true,
    is_system_default: true,
    config_json: {
      allowed_models: allowedModels,
      default_policy: {
        priority_mode: 'provider',
        scheduling_mode: 'cache_affinity',
      },
      model_policies: [],
      rules: [],
    },
    version: 1,
    created_at: 1,
    updated_at: 1,
    published_at: null,
    ...overrides,
  }
}

async function flushPromises(iterations = 5): Promise<void> {
  for (let index = 0; index < iterations; index += 1) {
    await Promise.resolve()
  }
  await nextTick()
}

async function mountPage(
  input: RoutingGroupRecord | RoutingGroupRecord[] = routingGroup(),
): Promise<void> {
  const groups = Array.isArray(input) ? input : [input]
  apiMocks.listRoutingGroups.mockResolvedValue({ items: groups, total: groups.length })
  apiMocks.getGlobalModels.mockResolvedValue({ models: [] })
  apiMocks.updateRoutingGroup.mockImplementation(
    async (groupId: string, payload: RoutingGroupUpdateRequest) => {
      const group = groups.find(item => item.id === groupId)
      if (!group) throw new Error(`unknown routing group: ${groupId}`)
      return {
        ...group,
        ...payload,
        config_json: payload.config_json ?? group.config_json,
        updated_at: 2,
      }
    },
  )

  root = document.createElement('div')
  document.body.appendChild(root)
  app = createApp(defineComponent({
    setup: () => () => h(RoutingProfiles),
  }))
  app.mount(root)
  await flushPromises()
}

function setTextareaValue(textarea: HTMLTextAreaElement, value: string): void {
  textarea.value = value
  textarea.dispatchEvent(new Event('input', { bubbles: true }))
}

beforeEach(() => {
  vi.clearAllMocks()
  if (!routeMocks.route) throw new Error('route mock was not initialized')
  routeMocks.route.name = 'RoutingProfileDetail'
  routeMocks.route.params = { groupId: 'group-1' }
})

afterEach(() => {
  app?.unmount()
  root?.remove()
  app = undefined
  root = undefined
})

describe('RoutingProfiles model allowlist', () => {
  it('saves one selector per line without an extra apply step', async () => {
    await mountPage()

    const textarea = root?.querySelector(
      '[data-testid="allowed-models-input"]',
    ) as HTMLTextAreaElement
    expect(textarea).toBeInstanceOf(HTMLTextAreaElement)

    setTextareaValue(textarea, 'gpt-5\nclaude-*\nvendor,model')
    await nextTick()

    const saveButton = root?.querySelector(
      'button[aria-label="保存"]',
    ) as HTMLButtonElement
    expect(saveButton.disabled).toBe(false)
    saveButton.click()
    await flushPromises()

    expect(apiMocks.updateRoutingGroup).toHaveBeenCalledWith(
      'group-1',
      expect.objectContaining({
        config_json: expect.objectContaining({
          allowed_models: ['gpt-5', 'claude-*', 'vendor,model'],
        }),
      }),
    )
  })

  it('locks the editor while a save is in flight', async () => {
    const group = routingGroup(['model-a'])
    let resolveUpdate: ((value: RoutingGroupRecord) => void) | undefined
    let submittedPayload: RoutingGroupUpdateRequest | undefined

    await mountPage(group)
    apiMocks.updateRoutingGroup.mockImplementationOnce(
      async (_groupId: string, payload: RoutingGroupUpdateRequest) => {
        submittedPayload = payload
        return await new Promise<RoutingGroupRecord>((resolve) => {
          resolveUpdate = resolve
        })
      },
    )

    const textarea = root?.querySelector(
      '[data-testid="allowed-models-input"]',
    ) as HTMLTextAreaElement
    setTextareaValue(textarea, 'model-a\nmodel-b')
    await nextTick()

    const saveButton = root?.querySelector(
      'button[aria-label="保存"]',
    ) as HTMLButtonElement
    saveButton.click()
    await nextTick()

    const editor = root?.querySelector('[aria-busy="true"]') as HTMLElement
    const clearButton = root?.querySelector(
      '[data-testid="clear-allowed-models"]',
    ) as HTMLButtonElement
    expect(editor.hasAttribute('inert')).toBe(true)
    expect(textarea.disabled).toBe(true)
    expect(clearButton.disabled).toBe(true)
    expect(saveButton.disabled).toBe(true)

    setTextareaValue(textarea, 'model-c')
    await nextTick()
    expect(submittedPayload?.config_json?.allowed_models).toEqual(['model-a', 'model-b'])

    if (!resolveUpdate || !submittedPayload) throw new Error('save request did not start')
    resolveUpdate({
      ...group,
      ...submittedPayload,
      config_json: submittedPayload.config_json ?? group.config_json,
      updated_at: 2,
    })
    await flushPromises()

    expect(root?.querySelector('[aria-busy="true"]')).toBeNull()
    expect((root?.querySelector(
      '[data-testid="allowed-models-input"]',
    ) as HTMLTextAreaElement).value).toBe('model-a\nmodel-b')
    expect(apiMocks.updateRoutingGroup).toHaveBeenCalledTimes(1)
  })

  it('keeps another group selected when an earlier save response arrives', async () => {
    const firstGroup = routingGroup(['model-a'], {
      id: 'group-1',
      name: 'First routing',
    })
    const secondGroup = routingGroup(['model-b'], {
      id: 'group-2',
      name: 'Second routing',
      is_system_default: false,
    })
    let resolveUpdate: ((value: RoutingGroupRecord) => void) | undefined
    let submittedPayload: RoutingGroupUpdateRequest | undefined

    await mountPage([firstGroup, secondGroup])
    apiMocks.updateRoutingGroup.mockImplementationOnce(
      async (_groupId: string, payload: RoutingGroupUpdateRequest) => {
        submittedPayload = payload
        return await new Promise<RoutingGroupRecord>((resolve) => {
          resolveUpdate = resolve
        })
      },
    )

    const textarea = root?.querySelector(
      '[data-testid="allowed-models-input"]',
    ) as HTMLTextAreaElement
    setTextareaValue(textarea, 'model-a\nmodel-a-new')
    await nextTick()
    ;(root?.querySelector('button[aria-label="保存"]') as HTMLButtonElement).click()
    await nextTick()

    if (!routeMocks.route) throw new Error('route mock was not initialized')
    routeMocks.route.params = { groupId: 'group-2' }
    await nextTick()
    expect((root?.querySelector(
      '[data-testid="allowed-models-input"]',
    ) as HTMLTextAreaElement).value).toBe('model-b')

    if (!resolveUpdate || !submittedPayload) throw new Error('save request did not start')
    resolveUpdate({
      ...firstGroup,
      ...submittedPayload,
      config_json: submittedPayload.config_json ?? firstGroup.config_json,
      updated_at: 2,
    })
    await flushPromises()

    expect(root?.querySelector('h2')?.textContent).toContain('Second routing')
    expect((root?.querySelector(
      '[data-testid="allowed-models-input"]',
    ) as HTMLTextAreaElement).value).toBe('model-b')
    expect(routeMocks.replace).not.toHaveBeenCalled()
  })

  it('refreshes a clean draft when returning to the saved group before the response arrives', async () => {
    const group = routingGroup(['model-a'])
    let resolveUpdate: ((value: RoutingGroupRecord) => void) | undefined
    let submittedPayload: RoutingGroupUpdateRequest | undefined

    await mountPage(group)
    apiMocks.updateRoutingGroup.mockImplementationOnce(
      async (_groupId: string, payload: RoutingGroupUpdateRequest) => {
        submittedPayload = payload
        return await new Promise<RoutingGroupRecord>((resolve) => {
          resolveUpdate = resolve
        })
      },
    )

    const textarea = root?.querySelector(
      '[data-testid="allowed-models-input"]',
    ) as HTMLTextAreaElement
    setTextareaValue(textarea, 'model-a\nmodel-b')
    await nextTick()
    ;(root?.querySelector('button[aria-label="保存"]') as HTMLButtonElement).click()
    await nextTick()

    if (!routeMocks.route) throw new Error('route mock was not initialized')
    routeMocks.route.name = 'RoutingProfiles'
    routeMocks.route.params = {}
    await nextTick()
    routeMocks.route.name = 'RoutingProfileDetail'
    routeMocks.route.params = { groupId: 'group-1' }
    await nextTick()
    expect((root?.querySelector(
      '[data-testid="allowed-models-input"]',
    ) as HTMLTextAreaElement).value).toBe('model-a')

    if (!resolveUpdate || !submittedPayload) throw new Error('save request did not start')
    resolveUpdate({
      ...group,
      ...submittedPayload,
      config_json: submittedPayload.config_json ?? group.config_json,
      updated_at: 2,
    })
    await flushPromises()

    expect((root?.querySelector(
      '[data-testid="allowed-models-input"]',
    ) as HTMLTextAreaElement).value).toBe('model-a\nmodel-b')
    expect((root?.querySelector(
      'button[aria-label="保存"]',
    ) as HTMLButtonElement).disabled).toBe(true)
  })

  it('does not attach an old create response to a recreated draft', async () => {
    if (!routeMocks.route) throw new Error('route mock was not initialized')
    routeMocks.route.name = 'RoutingProfileCreate'
    routeMocks.route.params = {}

    let resolveCreate: ((value: RoutingGroupRecord) => void) | undefined
    let submittedPayload: RoutingGroupCreateRequest | undefined
    await mountPage([])
    apiMocks.createRoutingGroup.mockImplementationOnce(
      async (payload: RoutingGroupCreateRequest) => {
        submittedPayload = payload
        return await new Promise<RoutingGroupRecord>((resolve) => {
          resolveCreate = resolve
        })
      },
    )

    ;(root?.querySelector('button[aria-label="保存"]') as HTMLButtonElement).click()
    await nextTick()

    routeMocks.route.name = 'RoutingProfiles'
    await nextTick()
    routeMocks.route.name = 'RoutingProfileCreate'
    await nextTick()
    expect(root?.querySelector('h2')?.textContent).toContain('新建调度策略')

    if (!resolveCreate || !submittedPayload) throw new Error('create request did not start')
    const config = submittedPayload.config_json
    resolveCreate({
      ...routingGroup(config?.allowed_models ?? [], {
        id: 'created-group',
        name: submittedPayload.name,
        description: submittedPayload.description,
        enabled: submittedPayload.enabled ?? false,
        is_system_default: submittedPayload.is_system_default ?? false,
      }),
      config_json: config ?? routingGroup().config_json,
    })
    await flushPromises()

    expect(root?.querySelector('h2')?.textContent).toContain('新建调度策略')
    expect(routeMocks.replace).not.toHaveBeenCalled()
    expect(apiMocks.createRoutingGroup).toHaveBeenCalledTimes(1)
  })
})
