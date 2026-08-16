use super::{
    classified, classified_with_request_auth_channel, detect_claude_client_surface,
    is_gemini_cli_request, is_gemini_models_route, is_gemini_operation_route, ClassifiedRoute,
};
use crate::ai_serving::ApiOperation;

pub(super) fn classify_ai_public_route(
    method: &http::Method,
    normalized_path: &str,
    headers: &http::HeaderMap,
) -> Option<ClassifiedRoute> {
    if method == http::Method::POST && normalized_path == "/v1/chat/completions" {
        Some(classified(
            "ai_public",
            "openai",
            "chat",
            "openai:chat",
            true,
        ))
    } else if method == http::Method::POST && normalized_path == "/v1/embeddings" {
        Some(classified(
            "ai_public",
            "openai",
            "embedding",
            "openai:embedding",
            true,
        ))
    } else if method == http::Method::POST && normalized_path == "/v1/rerank" {
        Some(classified(
            "ai_public",
            "openai",
            "rerank",
            "openai:rerank",
            true,
        ))
    } else if method == http::Method::POST
        && matches!(normalized_path, "/v1/responses" | "/v1/responses/compact")
    {
        if normalized_path.ends_with("/compact") {
            Some(classified(
                "ai_public",
                "openai",
                "responses:compact",
                "openai:responses:compact",
                true,
            ))
        } else {
            Some(classified(
                "ai_public",
                "openai",
                "responses",
                "openai:responses",
                true,
            ))
        }
    } else if method == http::Method::POST && normalized_path == "/v1/alpha/search" {
        Some(classified(
            "ai_public",
            "openai",
            "search",
            "openai:search",
            true,
        ))
    } else if method == http::Method::POST
        && matches!(
            normalized_path,
            "/v1/images/generations" | "/v1/images/edits"
        )
    {
        Some(classified(
            "ai_public",
            "openai",
            "image",
            "openai:image",
            true,
        ))
    } else if method == http::Method::POST && normalized_path == "/v1/messages/count_tokens" {
        let request_auth_channel = claude_request_auth_channel(headers);
        Some(
            classified_with_request_auth_channel(
                "ai_public",
                "claude",
                "count_tokens",
                request_auth_channel,
                "claude:messages",
                true,
            )
            .with_client_surface(detect_claude_client_surface(headers))
            .with_api_operation(ApiOperation::ClaudeCountTokens),
        )
    } else if method == http::Method::POST && normalized_path == "/v1/messages" {
        let request_auth_channel = claude_request_auth_channel(headers);
        Some(
            classified_with_request_auth_channel(
                "ai_public",
                "claude",
                "messages",
                request_auth_channel,
                "claude:messages",
                true,
            )
            .with_client_surface(detect_claude_client_surface(headers))
            .with_api_operation(ApiOperation::ClaudeMessagesCreate),
        )
    } else if normalized_path.starts_with("/v1/videos") {
        Some(classified(
            "ai_public",
            "openai",
            "video",
            "openai:video",
            true,
        ))
    } else if method == http::Method::POST
        && matches!(normalized_path, "/v1/interactions" | "/v1beta/interactions")
    {
        Some(classified_with_request_auth_channel(
            "ai_public",
            "gemini",
            "interactions",
            "api_key",
            "gemini:interactions",
            true,
        ))
    } else if method == http::Method::POST && is_gemini_models_route(normalized_path) {
        if normalized_path.ends_with(":predictLongRunning") {
            Some(classified(
                "ai_public",
                "gemini",
                "video",
                "gemini:video",
                true,
            ))
        } else if normalized_path.ends_with(":embedContent")
            || normalized_path.ends_with(":batchEmbedContents")
        {
            Some(classified_with_request_auth_channel(
                "ai_public",
                "gemini",
                "embedding",
                "api_key",
                "gemini:embedding",
                true,
            ))
        } else if is_gemini_cli_request(headers) {
            Some(classified_with_request_auth_channel(
                "ai_public",
                "gemini",
                "generate_content",
                "bearer_like",
                "gemini:generate_content",
                true,
            ))
        } else {
            Some(classified_with_request_auth_channel(
                "ai_public",
                "gemini",
                "generate_content",
                "api_key",
                "gemini:generate_content",
                true,
            ))
        }
    } else if is_gemini_operation_method(method, normalized_path)
        && is_gemini_operation_route(normalized_path)
    {
        Some(classified(
            "ai_public",
            "gemini",
            "video",
            "gemini:video",
            true,
        ))
    } else if is_gemini_files_method(method, normalized_path) {
        Some(classified(
            "ai_public",
            "gemini",
            "files",
            "gemini:files",
            true,
        ))
    } else {
        None
    }
}

fn claude_request_auth_channel(headers: &http::HeaderMap) -> &'static str {
    if crate::headers::header_value_str(headers, "x-api-key").is_some()
        || crate::headers::header_value_str(headers, "api-key").is_some()
    {
        "api_key"
    } else if crate::headers::header_value_str(headers, http::header::AUTHORIZATION.as_str())
        .is_some_and(|value| value.trim().to_ascii_lowercase().starts_with("bearer "))
    {
        "bearer_like"
    } else {
        "api_key"
    }
}

fn is_gemini_operation_method(method: &http::Method, normalized_path: &str) -> bool {
    method == http::Method::GET
        || (method == http::Method::POST && normalized_path.ends_with(":cancel"))
}

fn is_gemini_files_method(method: &http::Method, normalized_path: &str) -> bool {
    (method == http::Method::POST && normalized_path == "/upload/v1beta/files")
        || ((method == http::Method::GET || method == http::Method::DELETE)
            && normalized_path.starts_with("/v1beta/files"))
}
