import { describe, expect, it } from 'vitest'

import { MOCK_API_FORMATS, MOCK_GLOBAL_MODELS } from '../data'

describe('embedding mock metadata', () => {
  it('exposes embedding model metadata to frontend code without chat treatment', () => {
    const model = MOCK_GLOBAL_MODELS.find(item => item.name === 'text-embedding-3-small')

    expect(model).toMatchObject({
      supported_capabilities: ['embedding'],
      supports_embedding: true,
      config: {
        streaming: false,
        embedding: true,
        model_type: 'embedding',
        api_formats: ['openai:embedding'],
      },
    })
  })

  it('includes all embedding API formats as distinct catalog formats', () => {
    const embeddingFormats = MOCK_API_FORMATS.formats
      .filter(format => format.value.endsWith(':embedding') || format.value.endsWith('_embedding'))
      .map(format => [format.value, format.label])

    expect(embeddingFormats).toEqual([
      ['openai:embedding', 'OpenAI Embedding'],
      ['gemini:embedding', 'Gemini Embedding'],
    ])
  })

  it('includes rerank API formats as distinct catalog formats', () => {
    const rerankFormats = MOCK_API_FORMATS.formats
      .filter(format => format.value.endsWith(':rerank'))
      .map(format => [format.value, format.label])

    expect(rerankFormats).toEqual([
      ['openai:rerank', 'OpenAI Rerank'],
    ])
  })
})
