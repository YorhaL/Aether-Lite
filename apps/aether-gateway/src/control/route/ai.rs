use super::{
    classified, classified_with_request_auth_channel, detect_claude_client_surface,
    is_gemini_cli_request, is_gemini_models_route, ClassifiedRoute,
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
    } else if method == http::Method::GET
        && normalized_path == "/v1/realtime"
        && is_websocket_upgrade_request(headers)
    {
        Some(classified(
            "ai_public",
            "openai",
            "realtime",
            "openai:realtime",
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
        if normalized_path.ends_with(":embedContent")
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

fn is_websocket_upgrade_request(headers: &http::HeaderMap) -> bool {
    let has_upgrade_connection = headers
        .get(http::header::CONNECTION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .map(str::trim)
                .any(|value| value.eq_ignore_ascii_case("upgrade"))
        });
    let has_websocket_upgrade = headers
        .get(http::header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    has_upgrade_connection && has_websocket_upgrade
}

#[cfg(test)]
mod tests {
    use axum::http::header::{CONNECTION, UPGRADE};
    use axum::http::{HeaderMap, HeaderValue, Method};

    use super::classify_ai_public_route;

    #[test]
    fn realtime_requires_a_websocket_upgrade() {
        let mut headers = HeaderMap::new();
        headers.insert(CONNECTION, HeaderValue::from_static("keep-alive, Upgrade"));
        headers.insert(UPGRADE, HeaderValue::from_static("websocket"));

        let route = classify_ai_public_route(&Method::GET, "/v1/realtime", &headers)
            .expect("Realtime WebSocket should classify");
        assert_eq!(route.route_family, "openai");
        assert_eq!(route.route_kind, "realtime");
        assert_eq!(route.auth_endpoint_signature, "openai:realtime");

        assert!(
            classify_ai_public_route(&Method::GET, "/v1/realtime", &HeaderMap::new()).is_none()
        );
        assert!(classify_ai_public_route(&Method::POST, "/v1/realtime", &headers).is_none());
    }
}
