export type CcSwitchTargetApp = 'claude' | 'codex' | 'gemini' | 'opencode' | 'openclaw' | 'hermes'

export interface CcSwitchTargetOption {
  value: CcSwitchTargetApp
  label: string
  icon: string
}

export interface BuildCcSwitchProviderImportUrlInput {
  targetApp: CcSwitchTargetApp
  baseUrl: string
  apiKey: string
  apiKeyName: string
  siteName?: string
  modelId?: string
  modelIds?: CcSwitchModelIds
  providerName?: string
  enabled?: boolean
}

export interface CcSwitchModelIds {
  default?: string
  haiku?: string
  sonnet?: string
  opus?: string
}

interface NormalizedCcSwitchModelIds {
  default: string
  haiku: string
  sonnet: string
  opus: string
}

export const CC_SWITCH_TARGET_OPTIONS: CcSwitchTargetOption[] = [
  { value: 'claude', label: 'Claude Code', icon: 'claude' },
  { value: 'codex', label: 'Codex CLI', icon: 'openai' },
  { value: 'gemini', label: 'Gemini CLI', icon: 'gemini' },
  { value: 'opencode', label: 'OpenCode', icon: 'opencode' },
  { value: 'openclaw', label: 'OpenClaw', icon: 'openclaw' },
  { value: 'hermes', label: 'Hermes', icon: 'hermes' },
]

const AETHER_USAGE_QUERY_SCRIPT = [
  '({',
  '  request: {',
  '    url: "{{baseUrl}}/api/ccswitch/usage",',
  '    method: "GET",',
  '    headers: {',
  '      "Authorization": "Bearer {{apiKey}}",',
  '      "Accept": "application/json",',
  '      "User-Agent": "cc-switch/1.0"',
  '    }',
  '  },',
  '  extractor: function(response) {',
  '    if (response && response.is_valid === false) {',
  '      return {',
  '        isValid: false,',
  '        invalidMessage: response.invalid_message || "查询失败"',
  '      };',
  '    }',
  '',
  '    return {',
  '      isValid: true,',
  '      planName: response.plan_name || "Aether Lite",',
  '      remaining: response.remaining,',
  '      used: response.used,',
  '      total: response.total,',
  '      unit: response.unit || "USD",',
  '      extra: response.extra',
  '    };',
  '  }',
  '})',
].join('\n')

export function base64UrlEncodeUtf8(text: string): string {
  const bytes = new TextEncoder().encode(text)
  let binary = ''
  for (const byte of bytes) {
    binary += String.fromCharCode(byte)
  }

  return btoa(binary)
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/g, '')
}

function normalizedBaseUrl(baseUrl: string): string {
  return baseUrl.trim().replace(/\/+$/g, '')
}

function aetherV1BaseUrl(baseUrl: string): string {
  return `${normalizedBaseUrl(baseUrl)}/v1`
}

function ccSwitchEndpointForTarget(targetApp: CcSwitchTargetApp, baseUrl: string): string {
  if (targetApp === 'claude' || targetApp === 'gemini') {
    return normalizedBaseUrl(baseUrl)
  }
  return aetherV1BaseUrl(baseUrl)
}

function quoteTomlString(value: string): string {
  return `"${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`
}

export function ccSwitchTargetLabel(targetApp: CcSwitchTargetApp): string {
  return CC_SWITCH_TARGET_OPTIONS.find(option => option.value === targetApp)?.label || targetApp
}

export function defaultCcSwitchProviderName(
  siteName?: string,
): string {
  return siteName?.trim() || 'Aether Lite'
}

function buildCodexToml(baseUrl: string, modelId: string): string {
  return [
    'model_provider = "aether"',
    `model = ${quoteTomlString(modelId)}`,
    'model_reasoning_effort = "high"',
    'disable_response_storage = true',
    '',
    '[model_providers.aether]',
    'name = "Aether Lite"',
    `base_url = ${quoteTomlString(aetherV1BaseUrl(baseUrl))}`,
    'wire_api = "responses"',
    'requires_openai_auth = true',
    '',
  ].join('\n')
}

function normalizeCcSwitchModelIds(input: BuildCcSwitchProviderImportUrlInput): NormalizedCcSwitchModelIds {
  const defaultModel = input.modelIds?.default?.trim() || input.modelId?.trim() || ''
  const sonnet = input.modelIds?.sonnet?.trim() || defaultModel
  const haiku = input.modelIds?.haiku?.trim() || sonnet
  const opus = input.modelIds?.opus?.trim() || sonnet

  return {
    default: defaultModel,
    haiku,
    sonnet,
    opus,
  }
}

function buildCcSwitchConfig(input: BuildCcSwitchProviderImportUrlInput): Record<string, unknown> {
  const baseUrl = normalizedBaseUrl(input.baseUrl)
  const modelIds = normalizeCcSwitchModelIds(input)
  const providerName = input.providerName?.trim() || defaultCcSwitchProviderName(input.siteName)

  switch (input.targetApp) {
    case 'claude':
      return {
        env: {
          ANTHROPIC_AUTH_TOKEN: input.apiKey,
          ANTHROPIC_BASE_URL: baseUrl,
          ANTHROPIC_MODEL: modelIds.sonnet,
          ANTHROPIC_DEFAULT_HAIKU_MODEL: modelIds.haiku,
          ANTHROPIC_DEFAULT_SONNET_MODEL: modelIds.sonnet,
          ANTHROPIC_DEFAULT_OPUS_MODEL: modelIds.opus,
        },
      }
    case 'codex':
      return {
        auth: {
          OPENAI_API_KEY: input.apiKey,
        },
        config: buildCodexToml(baseUrl, modelIds.default),
      }
    case 'gemini':
      return {
        GEMINI_API_KEY: input.apiKey,
        GOOGLE_GEMINI_BASE_URL: baseUrl,
        GEMINI_MODEL: modelIds.default,
      }
    case 'opencode':
      return {
        npm: '@ai-sdk/openai-compatible',
        options: {
          baseURL: aetherV1BaseUrl(baseUrl),
          apiKey: input.apiKey,
        },
        models: {
          [modelIds.default]: {
            name: modelIds.default,
          },
        },
      }
    case 'openclaw':
      return {
        baseUrl: aetherV1BaseUrl(baseUrl),
        apiKey: input.apiKey,
        api: 'openai-completions',
        models: [{ id: modelIds.default, name: modelIds.default }],
      }
    case 'hermes':
      return {
        name: providerName,
        base_url: aetherV1BaseUrl(baseUrl),
        api_key: input.apiKey,
        api_mode: 'chat_completions',
        models: [{ id: modelIds.default, name: modelIds.default }],
      }
  }
}

export function buildCcSwitchProviderImportUrl(input: BuildCcSwitchProviderImportUrlInput): string {
  const providerName = input.providerName?.trim() || defaultCcSwitchProviderName(input.siteName)
  const icon = CC_SWITCH_TARGET_OPTIONS.find(option => option.value === input.targetApp)?.icon
  const modelIds = normalizeCcSwitchModelIds(input)
  const params = new URLSearchParams()

  params.set('resource', 'provider')
  params.set('app', input.targetApp)
  params.set('name', providerName)
  params.set('endpoint', ccSwitchEndpointForTarget(input.targetApp, input.baseUrl))
  params.set('apiKey', input.apiKey)
  params.set('model', input.targetApp === 'claude' ? modelIds.sonnet : modelIds.default)
  if (input.targetApp === 'claude') {
    params.set('haikuModel', modelIds.haiku)
    params.set('sonnetModel', modelIds.sonnet)
    params.set('opusModel', modelIds.opus)
  }
  params.set('configFormat', 'json')
  params.set('config', base64UrlEncodeUtf8(JSON.stringify(buildCcSwitchConfig(input))))
  params.set('enabled', input.enabled === false ? 'false' : 'true')
  params.set('usageEnabled', 'true')
  params.set('usageBaseUrl', normalizedBaseUrl(input.baseUrl))
  params.set('usageApiKey', input.apiKey)
  params.set('usageAutoInterval', '30')
  params.set('usageScript', base64UrlEncodeUtf8(AETHER_USAGE_QUERY_SCRIPT))
  if (icon) params.set('icon', icon)

  return `ccswitch://v1/import?${params.toString()}`
}
