import { describe, expect, it } from 'vitest'

import {
  API_FORMATS,
  apiFormatPermissionCovers,
  formatApiFormat,
  formatApiFormatShort,
  groupApiFormats,
  normalizeApiFormatAlias,
  sortApiFormats,
} from '@/api/endpoints/types'

describe('api format display helpers', () => {
  it('normalizes current enum-style names to canonical api format ids', () => {
    expect(normalizeApiFormatAlias('CLAUDE_MESSAGES')).toBe(API_FORMATS.CLAUDE_MESSAGES)
    expect(normalizeApiFormatAlias('OPENAI_RESPONSES')).toBe(API_FORMATS.OPENAI_RESPONSES)
    expect(normalizeApiFormatAlias('OPENAI_RESPONSES_COMPACT')).toBe(API_FORMATS.OPENAI_RESPONSES_COMPACT)
    expect(normalizeApiFormatAlias('OPENAI_REALTIME')).toBe(API_FORMATS.OPENAI_REALTIME)
    expect(normalizeApiFormatAlias('OPENAI_SEARCH')).toBe(API_FORMATS.OPENAI_SEARCH)
    expect(normalizeApiFormatAlias('SEARCH')).toBe(API_FORMATS.OPENAI_SEARCH)
    expect(normalizeApiFormatAlias('GEMINI_GENERATE_CONTENT')).toBe(API_FORMATS.GEMINI_GENERATE_CONTENT)
    expect(normalizeApiFormatAlias('OPENAI_EMBEDDING')).toBe(API_FORMATS.OPENAI_EMBEDDING)
    expect(normalizeApiFormatAlias('OPENAI_RERANK')).toBe(API_FORMATS.OPENAI_RERANK)
    expect(normalizeApiFormatAlias('GEMINI_INTERACTIONS')).toBe(API_FORMATS.GEMINI_INTERACTIONS)
    expect(normalizeApiFormatAlias('GEMINI_EMBEDDING')).toBe(API_FORMATS.GEMINI_EMBEDDING)
  })

  it('formats the OpenAI Realtime endpoint signature', () => {
    expect(formatApiFormat(API_FORMATS.OPENAI_REALTIME)).toBe('OpenAI Realtime')
    expect(formatApiFormatShort(API_FORMATS.OPENAI_REALTIME)).toBe('ORT')
  })

  it('formats rerank api format ids distinctly from chat formats', () => {
    expect(formatApiFormat(API_FORMATS.OPENAI_RERANK)).toBe('OpenAI Rerank')
    expect(formatApiFormatShort(API_FORMATS.OPENAI_RERANK)).toBe('ORR')
  })

  it('formats OpenAI Search as a first-class api format', () => {
    expect(formatApiFormat(API_FORMATS.OPENAI_SEARCH)).toBe('OpenAI Search')
    expect(formatApiFormatShort(API_FORMATS.OPENAI_SEARCH)).toBe('OS')
    expect(sortApiFormats([
      API_FORMATS.OPENAI_EMBEDDING,
      API_FORMATS.OPENAI_SEARCH,
      API_FORMATS.OPENAI_RESPONSES,
    ])).toEqual([
      API_FORMATS.OPENAI_RESPONSES,
      API_FORMATS.OPENAI_SEARCH,
      API_FORMATS.OPENAI_EMBEDDING,
    ])
  })

  it('applies Responses companion permissions in one direction', () => {
    expect(apiFormatPermissionCovers('OPENAI_RESPONSES', 'openai:search')).toBe(true)
    expect(apiFormatPermissionCovers('OPENAI_RESPONSES', 'openai:responses:compact')).toBe(true)
    expect(apiFormatPermissionCovers('openai:search', 'openai:responses')).toBe(false)
    expect(apiFormatPermissionCovers('openai:responses:compact', 'openai:responses')).toBe(false)
  })

  it('formats embedding api format ids distinctly from chat formats', () => {
    expect(formatApiFormat(API_FORMATS.GEMINI_INTERACTIONS)).toBe('Gemini Interactions')
    expect(formatApiFormatShort(API_FORMATS.GEMINI_INTERACTIONS)).toBe('GI')
    expect(formatApiFormat(API_FORMATS.OPENAI_EMBEDDING)).toBe('OpenAI Embedding')
    expect(formatApiFormat(API_FORMATS.GEMINI_EMBEDDING)).toBe('Gemini Embedding')
    expect(formatApiFormatShort(API_FORMATS.OPENAI_EMBEDDING)).toBe('OE')
    expect(formatApiFormatShort(API_FORMATS.GEMINI_EMBEDDING)).toBe('GE')
  })

  it('does not remap retired api format ids', () => {
    expect(normalizeApiFormatAlias('openai:cli')).toBe('openai:cli')
    expect(formatApiFormat('openai:cli')).toBe('openai:cli')
    expect(formatApiFormatShort('openai:cli')).toBe('op')

    expect(normalizeApiFormatAlias('openai:compact')).toBe('openai:compact')
    expect(formatApiFormat('openai:compact')).toBe('openai:compact')
    expect(formatApiFormatShort('openai:compact')).toBe('op')
  })

  it('does not remap retired enum-style aliases', () => {
    expect(normalizeApiFormatAlias('OPENAI_CLI')).toBe('openai_cli')
    expect(formatApiFormat('OPENAI_CLI')).toBe('openai_cli')
    expect(formatApiFormatShort('OPENAI_CLI')).toBe('op')

    expect(normalizeApiFormatAlias('OPENAI_COMPACT')).toBe('openai_compact')
    expect(formatApiFormat('OPENAI_COMPACT')).toBe('openai_compact')
    expect(formatApiFormatShort('OPENAI_COMPACT')).toBe('op')
  })

  it('sorts only current canonical formats into known slots', () => {
    expect(sortApiFormats([
      'openai:compact',
      API_FORMATS.OPENAI,
      API_FORMATS.OPENAI_RESPONSES,
      API_FORMATS.OPENAI_EMBEDDING,
      API_FORMATS.OPENAI_RERANK,
      API_FORMATS.GEMINI_INTERACTIONS,
      API_FORMATS.GEMINI_EMBEDDING,
    ])).toEqual([
      API_FORMATS.OPENAI,
      API_FORMATS.OPENAI_RESPONSES,
      API_FORMATS.OPENAI_EMBEDDING,
      API_FORMATS.OPENAI_RERANK,
      API_FORMATS.GEMINI_INTERACTIONS,
      API_FORMATS.GEMINI_EMBEDDING,
      'openai:compact',
    ])
  })

  it('keeps embedding formats after chat/generation formats within each family', () => {
    expect(sortApiFormats([
      API_FORMATS.GEMINI_EMBEDDING,
      API_FORMATS.OPENAI_EMBEDDING,
      API_FORMATS.OPENAI_RERANK,
      API_FORMATS.GEMINI_GENERATE_CONTENT,
      API_FORMATS.GEMINI_INTERACTIONS,
      API_FORMATS.OPENAI,
    ])).toEqual([
      API_FORMATS.OPENAI,
      API_FORMATS.OPENAI_EMBEDDING,
      API_FORMATS.OPENAI_RERANK,
      API_FORMATS.GEMINI_GENERATE_CONTENT,
      API_FORMATS.GEMINI_INTERACTIONS,
      API_FORMATS.GEMINI_EMBEDDING,
    ])
  })

  it('groups embedding api formats by provider family', () => {
    expect(groupApiFormats([
      API_FORMATS.GEMINI_EMBEDDING,
      API_FORMATS.OPENAI_EMBEDDING,
      API_FORMATS.OPENAI_RERANK,
    ])).toEqual([
      { family: 'openai', label: 'OpenAI', formats: [API_FORMATS.OPENAI_EMBEDDING, API_FORMATS.OPENAI_RERANK] },
      { family: 'gemini', label: 'Gemini', formats: [API_FORMATS.GEMINI_EMBEDDING] },
    ])
  })

  it('groups retired enum-style aliases as unknown raw families', () => {
    expect(groupApiFormats(['OPENAI_CLI'])).toEqual([{
      family: 'openai_cli',
      label: 'openai_cli',
      formats: ['OPENAI_CLI'],
    }])
  })
})
