import { describe, expect, it } from 'vitest'

import {
  DEFAULT_ROUTING_POLICY_MODEL,
  allowedModelsMirrorPerModelPolicies,
  clearAllowedModels,
  copyPerModelRoutingConfig,
  createEmptyModelPolicy,
  createEmptyRoutingGroupConfig,
  formatAllowedModelsInput,
  getDefaultModelPolicy,
  getModelScheduling,
  modelSchedulingRuleId,
  normalizeRoutingGroupConfig,
  parseAllowedModelsInput,
  removePerModelRoutingConfig,
  routingModelScopeLabel,
  savePerModelRoutingConfig,
  setDefaultProviderPriorityOverrides,
  setRoutingSortingScope,
  updateAllowedModelsFromInput,
  upsertModelSchedulingRule,
  upsertModelPolicy,
} from '../utils/routingPolicy'
import { sortCandidateTraces, summarizeRoutingTrace, type RoutingDecisionTrace } from '../utils/routingTrace'

describe('routingPolicy', () => {
  it('normalizes partial configs with stable defaults', () => {
    const config = normalizeRoutingGroupConfig({
      allowed_models: ['gpt-5'],
    })

    expect(config.default_policy.priority_mode).toBe('provider')
    expect(config.default_policy.scheduling_mode).toBe('cache_affinity')
    expect(config.allowed_models).toEqual(['gpt-5'])
  })

  it('upserts model policies by model name', () => {
    const config = createEmptyRoutingGroupConfig()
    const next = upsertModelPolicy(config, {
      ...createEmptyModelPolicy('gpt-5'),
      allowed_providers: ['provider-a'],
    })

    expect(next.model_policies).toHaveLength(1)
    expect(next.model_policies[0].allowed_providers).toEqual(['provider-a'])
  })

  it('stores default priority overrides on the wildcard model policy', () => {
    const config = upsertModelPolicy(createEmptyRoutingGroupConfig(), createEmptyModelPolicy('gpt-5'))
    const next = setDefaultProviderPriorityOverrides(config, {
      'provider-a': 0,
      'provider-b': 2,
    })

    const policy = getDefaultModelPolicy(next)
    expect(policy.model).toBe(DEFAULT_ROUTING_POLICY_MODEL)
    expect(next.model_policies.map(item => item.model)).toEqual([DEFAULT_ROUTING_POLICY_MODEL, 'gpt-5'])
    expect(policy.provider_priority_overrides).toEqual({
      'provider-a': 0,
      'provider-b': 2,
    })
  })

  it('stores per-model scheduling as generated routing rules', () => {
    const next = upsertModelSchedulingRule(createEmptyRoutingGroupConfig(), 'gpt-5', {
      priority_mode: 'global_key',
      scheduling_mode: 'fixed_order',
    })

    expect(next.rules).toHaveLength(1)
    expect(next.rules[0].id).toBe(modelSchedulingRuleId('gpt-5'))
    expect(next.rules[0].conditions).toEqual({
      field: 'model',
      op: 'eq',
      value: 'gpt-5',
    })
    expect(getModelScheduling(next, 'gpt-5')).toMatchObject({
      priority_mode: 'global_key',
      scheduling_mode: 'fixed_order',
    })
  })

  it('updates the model allowlist only through explicit scope controls', () => {
    const config = normalizeRoutingGroupConfig({
      allowed_models: ['legacy-model'],
    })

    expect(parseAllowedModelsInput(' gpt-5\nclaude-*\nlegacy-model\ngpt-5 ')).toEqual([
      'gpt-5',
      'claude-*',
      'legacy-model',
    ])

    const restricted = updateAllowedModelsFromInput(
      config,
      'gpt-5\nclaude-*\nlegacy-model\ngpt-5',
    )
    expect(restricted.allowed_models).toEqual(['gpt-5', 'claude-*', 'legacy-model'])
    expect(formatAllowedModelsInput(restricted.allowed_models)).toBe('gpt-5\nclaude-*\nlegacy-model')
    expect(routingModelScopeLabel(restricted)).toBe('3 个模型')

    const unrestricted = clearAllowedModels(restricted)
    expect(unrestricted.allowed_models).toEqual([])
    expect(routingModelScopeLabel(unrestricted)).toBe('全部模型')
  })

  it('round-trips selectors containing commas and labels wildcard scope as unrestricted', () => {
    const selectors = ['vendor,model', 'gpt-*']
    expect(parseAllowedModelsInput(formatAllowedModelsInput(selectors))).toEqual(selectors)

    const wildcard = normalizeRoutingGroupConfig({ allowed_models: ['gpt-*', '*'] })
    expect(routingModelScopeLabel(wildcard)).toBe('全部模型')
  })

  it('preserves historical empty selectors until unrestricted scope is explicit', () => {
    const legacy = normalizeRoutingGroupConfig({ allowed_models: ['', '  '] })

    expect(updateAllowedModelsFromInput(legacy, '  \n')).toMatchObject({
      allowed_models: ['', '  '],
    })
    expect(clearAllowedModels(legacy).allowed_models).toEqual([])
  })

  it('preserves an explicit model allowlist across per-model editing actions', () => {
    const allowlist = ['gpt-*', 'legacy-model']
    let config = normalizeRoutingGroupConfig({
      allowed_models: allowlist,
      model_policies: [{
        ...createEmptyModelPolicy('special-model'),
        allowed_providers: ['provider-special'],
      }],
    })
    config = upsertModelSchedulingRule(config, 'special-model', {
      priority_mode: 'global_key',
      scheduling_mode: 'fixed_order',
    })

    const perModel = setRoutingSortingScope(config, 'per_model')
    expect(perModel.allowed_models).toEqual(allowlist)
    expect(getModelScheduling(perModel, 'special-model')).toMatchObject({
      priority_mode: 'global_key',
      scheduling_mode: 'fixed_order',
    })

    const saved = savePerModelRoutingConfig(perModel, 'new-special-model')
    expect(saved.allowed_models).toEqual(allowlist)
    expect(saved.model_policies.map(policy => policy.model)).toContain('new-special-model')

    const copied = copyPerModelRoutingConfig(
      saved,
      saved,
      'special-model',
      'copied-special-model',
    )
    expect(copied.allowed_models).toEqual(allowlist)
    expect(copied.model_policies.find(policy => policy.model === 'copied-special-model'))
      .toMatchObject({ allowed_providers: ['provider-special'] })
    expect(getModelScheduling(copied, 'copied-special-model')).toMatchObject({
      priority_mode: 'global_key',
      scheduling_mode: 'fixed_order',
    })

    const removed = removePerModelRoutingConfig(copied, 'special-model')
    expect(removed.allowed_models).toEqual(allowlist)
    expect(removed.model_policies.map(policy => policy.model)).not.toContain('special-model')
    expect(removed.rules.map(rule => rule.id)).not.toContain(modelSchedulingRuleId('special-model'))

    const unified = setRoutingSortingScope(removed, 'unified')
    expect(unified.allowed_models).toEqual(allowlist)
    expect(unified.model_policies.filter(policy => policy.model !== DEFAULT_ROUTING_POLICY_MODEL))
      .toEqual([])
    expect(unified.rules.some(rule => rule.id.startsWith('ui_model_scheduling:'))).toBe(false)
  })

  it('recognizes legacy allowlist mirrors without mutating historical values', () => {
    const config = normalizeRoutingGroupConfig({
      allowed_models: [' model-b ', 'model-a', 'model-a'],
      model_policies: [
        createEmptyModelPolicy('model-a'),
        createEmptyModelPolicy('model-b'),
      ],
    })

    expect(allowedModelsMirrorPerModelPolicies(config)).toBe(true)
    expect(config.allowed_models).toEqual([' model-b ', 'model-a', 'model-a'])
    expect(allowedModelsMirrorPerModelPolicies({
      ...config,
      allowed_models: ['model-*'],
    })).toBe(false)
  })
})

describe('routingTrace', () => {
  it('sorts candidate traces by selected order', () => {
    const sorted = sortCandidateTraces([
      candidate('provider-b', 2),
      candidate('provider-a', 1),
    ])

    expect(sorted.map(item => item.provider_id)).toEqual(['provider-a', 'provider-b'])
  })

  it('summarizes trace metadata', () => {
    const trace: RoutingDecisionTrace = {
      group_id: 'group-a',
      group_version: 3,
      selection_source: 'explicit',
      selected_rules: ['rule-a'],
      original_model: 'gpt-5',
      resolved_model: 'gpt-5',
      client_api_format: 'openai:chat',
      global_candidates: [candidate('provider-a', 0)],
      runtime_facts: {},
    }

    expect(summarizeRoutingTrace(trace)).toContain('分组: group-a')
    expect(summarizeRoutingTrace(trace)).toContain('候选: 1')
  })
})

function candidate(providerId: string, selectedOrder: number) {
  return {
    candidate_kind: 'provider' as const,
    provider_id: providerId,
    endpoint_id: `${providerId}-endpoint`,
    model_id: 'model-a',
    key_id: `${providerId}-key`,
    selected_order: selectedOrder,
    ranking_vector: {
      provider_priority_before: selectedOrder,
      provider_priority_after: selectedOrder,
      key_priority_before: selectedOrder,
      key_priority_after: selectedOrder,
    },
  }
}
