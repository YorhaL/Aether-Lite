import { describe, expect, it } from 'vitest'

import { getDefaultEndpointBaseUrl, getDefaultEndpointPath } from '../endpoint-default-paths'

const apiFormats = [
  { value: 'openai:chat', default_path: '/v1/chat/completions' },
  { value: 'gemini:generate_content', default_path: '/v1beta/models/{model}:{action}' },
  { value: 'gemini:interactions', default_path: '/v1/interactions' },
  { value: 'gemini:embedding', default_path: '/v1beta/models/{model}:embedContent' },
  { value: 'openai:responses', default_path: '/v1/responses' },
  { value: 'openai:search', default_path: '/v1/alpha/search' },
  { value: 'openai:embedding', default_path: '/v1/embeddings' },
  { value: 'openai:rerank', default_path: '/v1/rerank' },
  { value: 'openai:image', default_path: '/v1/images/generations' },
  { value: 'jina:embedding', default_path: '/v1/embeddings' },
  { value: 'jina:rerank', default_path: '/v1/rerank' },
  { value: 'claude:messages', default_path: '/v1/messages' },
]

describe('endpoint default paths', () => {
  it('uses Gemini Developer API resource paths for custom Gemini endpoints', () => {
    expect(getDefaultEndpointPath({
      apiFormat: 'gemini:generate_content',
      apiFormats,
    })).toBe('/models/{model}:{action}')

    expect(getDefaultEndpointPath({
      apiFormat: 'gemini:embedding',
      apiFormats,
    })).toBe('/models/{model}:embedContent')

    expect(getDefaultEndpointPath({
      apiFormat: 'gemini:interactions',
      apiFormats,
    })).toBe('/interactions')

  })

  it('uses the Search path relative to the configured API root', () => {
    expect(getDefaultEndpointBaseUrl({
      apiFormat: 'openai:search',
      baseUrl: 'https://api.openai.com',
    })).toBe('https://api.openai.com/v1')
    expect(getDefaultEndpointPath({
      apiFormat: 'openai:search',
      baseUrl: 'https://api.openai.com/v1',
      apiFormats,
    })).toBe('/alpha/search')

  })

  it('drops /v1 from API-root defaults because base URL is the API root', () => {
    expect(getDefaultEndpointPath({
      apiFormat: 'openai:chat',
      baseUrl: 'https://proxy.example.com/api',
      apiFormats,
    })).toBe('/chat/completions')

    expect(getDefaultEndpointPath({
      apiFormat: 'openai:embedding',
      baseUrl: 'https://proxy.example.com/api?tenant=demo',
      apiFormats,
    })).toBe('/embeddings')

    expect(getDefaultEndpointPath({
      apiFormat: 'openai:rerank',
      baseUrl: 'https://proxy.example.com/api?tenant=demo',
      apiFormats,
    })).toBe('/rerank')

    expect(getDefaultEndpointPath({
      apiFormat: 'openai:image',
      baseUrl: 'https://proxy.example.com/api',
      apiFormats,
    })).toBe('/images/generations')

    expect(getDefaultEndpointPath({
      apiFormat: 'jina:embedding',
      baseUrl: 'https://api.jina.ai/v1',
      apiFormats,
    })).toBe('/embeddings')

    expect(getDefaultEndpointPath({
      apiFormat: 'jina:rerank',
      baseUrl: 'https://api.jina.ai/v1',
      apiFormats,
    })).toBe('/rerank')

    expect(getDefaultEndpointPath({
      apiFormat: 'openai:chat',
      baseUrl: 'https://proxy.example.com/openai',
      apiFormats,
    })).toBe('/chat/completions')

    expect(getDefaultEndpointPath({
      apiFormat: 'openai:chat',
      baseUrl: 'https://proxy.example.com',
      apiFormats,
    })).toBe('/chat/completions')
  })

  it('drops /v1 from OpenAI-compatible defaults when base URL already includes a known API root', () => {
    expect(getDefaultEndpointPath({
      apiFormat: 'openai:chat',
      baseUrl: 'https://open.bigmodel.cn/api/coding/paas/v4',
      apiFormats,
    })).toBe('/chat/completions')

    expect(getDefaultEndpointPath({
      apiFormat: 'openai:responses',
      baseUrl: 'https://api.openai.example/v1',
      apiFormats,
    })).toBe('/responses')
  })

  it('drops /v1 from Claude Messages defaults because base URL is the API root', () => {
    expect(getDefaultEndpointPath({
      apiFormat: 'claude:messages',
      baseUrl: 'https://api.anthropic.example/v1',
      apiFormats,
    })).toBe('/messages')

    expect(getDefaultEndpointPath({
      apiFormat: 'claude:messages',
      baseUrl: 'https://proxy.example.com/api',
      apiFormats,
    })).toBe('/messages')

    expect(getDefaultEndpointPath({
      apiFormat: 'claude:messages',
      baseUrl: 'https://proxy.example.com/anthropic',
      apiFormats,
    })).toBe('/messages')
  })

  it('defaults API-root base URLs to the format version when using a provider website', () => {
    expect(getDefaultEndpointBaseUrl({
      apiFormat: 'openai:chat',
      baseUrl: 'https://api.openai.com',
    })).toBe('https://api.openai.com/v1')

    expect(getDefaultEndpointBaseUrl({
      apiFormat: 'openai:responses',
      baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode',
    })).toBe('https://dashscope.aliyuncs.com/compatible-mode/v1')

    expect(getDefaultEndpointBaseUrl({
      apiFormat: 'claude:messages',
      baseUrl: 'https://api.anthropic.com',
    })).toBe('https://api.anthropic.com/v1')

    expect(getDefaultEndpointBaseUrl({
      apiFormat: 'openai:embedding',
      baseUrl: 'https://api.openai.com',
    })).toBe('https://api.openai.com/v1')

    expect(getDefaultEndpointBaseUrl({
      apiFormat: 'openai:image',
      baseUrl: 'https://api.openai.com',
    })).toBe('https://api.openai.com/v1')

    expect(getDefaultEndpointBaseUrl({
      apiFormat: 'jina:embedding',
      baseUrl: 'https://api.jina.ai',
    })).toBe('https://api.jina.ai/v1')

    expect(getDefaultEndpointBaseUrl({
      apiFormat: 'gemini:generate_content',
      baseUrl: 'https://generativelanguage.googleapis.com',
    })).toBe('https://generativelanguage.googleapis.com/v1beta')

    expect(getDefaultEndpointBaseUrl({
      apiFormat: 'gemini:embedding',
      baseUrl: 'https://generativelanguage.googleapis.com',
    })).toBe('https://generativelanguage.googleapis.com/v1beta')

    expect(getDefaultEndpointBaseUrl({
      apiFormat: 'gemini:interactions',
      baseUrl: 'https://generativelanguage.googleapis.com',
    })).toBe('https://generativelanguage.googleapis.com/v1')

    expect(getDefaultEndpointBaseUrl({
      apiFormat: 'openai:chat',
      baseUrl: 'https://open.bigmodel.cn/api/coding/paas/v4',
    })).toBe('https://open.bigmodel.cn/api/coding/paas/v4')

    expect(getDefaultEndpointBaseUrl({
      apiFormat: 'openai:chat',
      baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai',
    })).toBe('https://generativelanguage.googleapis.com/v1beta/openai')

    expect(getDefaultEndpointBaseUrl({
      apiFormat: 'openai:chat',
      baseUrl: 'https://gateway.example.com',
    })).toBe('https://gateway.example.com/v1')
  })
})
