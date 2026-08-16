import type { PublicGlobalModel } from '@/api/public-models'

function isEmbeddingApiFormat(format: unknown): boolean {
  const value = String(format).trim().toLowerCase()
  return value.endsWith(':embedding') || value === 'aliyun:multimodal_embedding'
}

export function supportsEmbedding(model: PublicGlobalModel): boolean {
  return model.supports_embedding === true
    || model.supported_capabilities?.includes('embedding') === true
    || model.config?.embedding === true
    || model.config?.model_type === 'embedding'
    || (Array.isArray(model.config?.api_formats) && model.config.api_formats.some(isEmbeddingApiFormat))
}

export function supportsRerank(model: PublicGlobalModel): boolean {
  return model.supported_capabilities?.includes('rerank') === true
    || model.config?.rerank === true
    || model.config?.model_type === 'rerank'
    || (Array.isArray(model.config?.api_formats) && model.config.api_formats.some((format) => String(format).endsWith(':rerank')))
}

export function getModelCapabilityLabels(model: PublicGlobalModel): string[] {
  const labels: string[] = []
  if (supportsRerank(model)) {
    labels.push('Rerank')
  } else if (supportsEmbedding(model)) {
    labels.push('Embedding')
  } else {
    labels.push('Chat')
  }
  if (model.config?.image_generation === true) labels.push('Image')
  return labels
}
