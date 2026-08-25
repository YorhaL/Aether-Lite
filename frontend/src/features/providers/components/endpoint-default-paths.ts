interface ApiFormatPathDefinition {
  value: string
  default_path: string
}

export function normalizeEndpointApiFormat(apiFormat: string): string {
  switch (apiFormat.trim().toLowerCase()) {
    default:
      return apiFormat.trim().toLowerCase()
  }
}

function parseBaseUrlParts(baseUrl?: string | null): { host: string; path: string } | null {
  const raw = (baseUrl || '').trim()
  if (!raw) return null
  try {
    const parsed = new URL(raw)
    return {
      host: parsed.hostname.toLowerCase(),
      path: parsed.pathname.replace(/\/+$/, '').toLowerCase(),
    }
  } catch {
    const pathStart = raw.indexOf('/')
    return {
      host: '',
      path: pathStart >= 0 ? raw.slice(pathStart).split('?')[0].replace(/\/+$/, '').toLowerCase() : '',
    }
  }
}

function baseUrlHasPathApiRoot(baseUrl?: string | null): boolean {
  const path = parseBaseUrlParts(baseUrl)?.path
  return !!path && path !== '/'
}

function baseUrlEndsWithV1Root(baseUrl?: string | null): boolean {
  return parseBaseUrlParts(baseUrl)?.path.endsWith('/v1') ?? false
}

function baseUrlHasVersionedApiRoot(baseUrl?: string | null): boolean {
  const path = parseBaseUrlParts(baseUrl)?.path || ''
  return /\/v\d+(?:beta\d*)?(?:\/|$)/i.test(path)
}

function openAiCompatibleBaseIncludesApiRoot(baseUrl?: string | null): boolean {
  return baseUrlEndsWithV1Root(baseUrl)
    || baseUrlHasPathApiRoot(baseUrl)
}

function stripVersionPrefixForApiRoot(path: string): string {
  return path.replace(/^\/v\d+(?:beta\d*)?(?=\/)/i, '')
}

function isOpenAiCompatibleFormat(apiFormat: string): boolean {
  return apiFormat.startsWith('openai:')
}

function usesVersionedApiRootByDefault(apiFormat: string): boolean {
  return apiFormat === 'openai:chat'
    || apiFormat === 'openai:responses'
    || apiFormat === 'openai:responses:compact'
    || apiFormat === 'openai:realtime'
    || apiFormat === 'openai:search'
    || apiFormat === 'openai:embedding'
    || apiFormat === 'openai:rerank'
    || apiFormat === 'openai:image'
    || apiFormat === 'claude:messages'
    || apiFormat === 'gemini:generate_content'
    || apiFormat === 'gemini:interactions'
    || apiFormat === 'gemini:embedding'
}

function versionedApiRootSuffix(apiFormat: string): '/v1' | '/v1beta' {
  if (
    apiFormat === 'gemini:interactions'
  ) {
    return '/v1'
  }
  if (
    apiFormat === 'gemini:generate_content'
    || apiFormat === 'gemini:embedding'
  ) {
    return '/v1beta'
  }
  return '/v1'
}

function appendVersionedApiRoot(baseUrl: string, suffix: '/v1' | '/v1beta'): string {
  const raw = baseUrl.trim()
  if (!raw) return ''
  try {
    const parsed = new URL(raw)
    parsed.pathname = `${parsed.pathname.replace(/\/+$/, '')}${suffix}`
    return parsed.toString().replace(/\/$/, '')
  } catch {
    const [base, query] = raw.split('?', 2)
    const normalizedBase = base.replace(/\/+$/, '')
    return query === undefined ? `${normalizedBase}${suffix}` : `${normalizedBase}${suffix}?${query}`
  }
}

export function getDefaultEndpointBaseUrl(params: {
  apiFormat: string
  baseUrl?: string | null
}): string {
  const normalizedApiFormat = normalizeEndpointApiFormat(params.apiFormat)
  const rawBaseUrl = (params.baseUrl || '').trim()
  if (!rawBaseUrl) return ''
  if (
    usesVersionedApiRootByDefault(normalizedApiFormat)
    && !baseUrlHasVersionedApiRoot(rawBaseUrl)
  ) {
    return appendVersionedApiRoot(rawBaseUrl, versionedApiRootSuffix(normalizedApiFormat))
  }
  return rawBaseUrl
}

export function getDefaultEndpointPath(params: {
  apiFormat: string
  baseUrl?: string
  apiFormats: ApiFormatPathDefinition[]
}): string {
  const normalizedApiFormat = normalizeEndpointApiFormat(params.apiFormat)

  const format = params.apiFormats.find(f => f.value === normalizedApiFormat)
  const defaultPath = format?.default_path || ''
  if (usesVersionedApiRootByDefault(normalizedApiFormat)) {
    return stripVersionPrefixForApiRoot(defaultPath)
  }
  if (openAiCompatibleBaseIncludesApiRoot(params.baseUrl) && isOpenAiCompatibleFormat(normalizedApiFormat)) {
    return stripVersionPrefixForApiRoot(defaultPath)
  }
  return defaultPath
}
