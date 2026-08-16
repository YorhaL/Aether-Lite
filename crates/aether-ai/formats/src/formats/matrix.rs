use crate::formats::id::normalize_api_format_alias;

pub fn is_embedding_api_format(api_format: &str) -> bool {
    matches!(
        normalize_api_format_alias(api_format).as_str(),
        "openai:embedding"
            | "gemini:embedding"
            | "jina:embedding"
            | "doubao:embedding"
            | "aliyun:multimodal_embedding"
    )
}

pub fn is_rerank_api_format(api_format: &str) -> bool {
    matches!(
        normalize_api_format_alias(api_format).as_str(),
        "openai:rerank" | "jina:rerank"
    )
}

pub fn is_gemini_interactions_api_format(api_format: &str) -> bool {
    normalize_api_format_alias(api_format) == "gemini:interactions"
}
