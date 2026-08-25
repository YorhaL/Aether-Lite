use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, HeaderValue, Response, StatusCode};
use axum::routing::{any, get, post};
use axum::Router;

use super::{claude, gemini, openai};
use crate::api::response::build_local_http_error_response_with_request_path;
use crate::headers::extract_or_generate_trace_id;
use crate::{
    handlers::proxy::{proxy_request, realtime_websocket},
    state::AppState,
    GatewayError,
};

// Router registration patterns live here so AI public ingress has a single mount registry.
const AI_POST_ROUTE_PATTERNS: &[&str] = &[
    "/v1/chat/completions",
    "/v1/embeddings",
    "/v1/rerank",
    "/v1/responses",
    "/v1/responses/compact",
    "/v1/alpha/search",
    "/v1/images/generations",
    "/v1/images/edits",
    "/v1/interactions",
    "/v1beta/interactions",
    "/v1internal:loadCodeAssist",
    "/v1internal:fetchAvailableModels",
    "/v1internal:retrieveUserQuotaSummary",
    "/v1internal:fetchUserInfo",
    "/v1internal:fetchAdminControls",
    "/v1internal:setUserSettings",
    "/v1internal:listExperiments",
    "/v1internal:recordCodeAssistMetrics",
    "/v1internal:writeTrajectoryAcls",
    "/v1internal:streamGenerateContent",
];

const CLAUDE_POST_ROUTE_PATTERNS: &[&str] = &["/v1/messages", "/v1/messages/count_tokens"];

const AI_ANY_ROUTE_PATTERNS: &[&str] =
    &["/v1/models/{*gemini_path}", "/v1beta/models/{*gemini_path}"];

pub(crate) fn mount_ai_routes(mut router: Router<AppState>) -> Router<AppState> {
    router = router.route("/v1/realtime", get(realtime_websocket));
    for path in AI_POST_ROUTE_PATTERNS {
        router = router.route(path, post(proxy_request));
    }
    for path in CLAUDE_POST_ROUTE_PATTERNS {
        router = router.route(
            path,
            post(proxy_request).fallback(claude_method_not_allowed),
        );
    }
    for path in AI_ANY_ROUTE_PATTERNS {
        router = router.route(path, any(proxy_request));
    }
    router
}

async fn claude_method_not_allowed(request: Request) -> Result<Response<Body>, GatewayError> {
    let trace_id = extract_or_generate_trace_id(request.headers());
    let mut response = build_local_http_error_response_with_request_path(
        &trace_id,
        None,
        Some(request.uri().path()),
        StatusCode::METHOD_NOT_ALLOWED,
        "Method not allowed",
    )?;
    response
        .headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static("POST"));
    Ok(response)
}

pub(crate) fn public_api_format_local_path(api_format: &str) -> &'static str {
    let normalized = api_format.trim().to_ascii_lowercase();
    openai::local_path(&normalized)
        .or_else(|| claude::local_path(&normalized))
        .or_else(|| gemini::local_path(&normalized))
        .unwrap_or("/")
}

pub(crate) fn normalize_admin_endpoint_signature(api_format: &str) -> Option<&'static str> {
    let normalized = api_format.trim().to_ascii_lowercase();
    openai::normalized_signature(&normalized)
        .or_else(|| claude::normalized_signature(&normalized))
        .or_else(|| gemini::normalized_signature(&normalized))
}

pub(crate) fn admin_endpoint_signature_parts(
    api_format: &str,
) -> Option<(&'static str, &'static str, &'static str)> {
    let normalized = normalize_admin_endpoint_signature(api_format)?;
    let (api_family, endpoint_kind) = normalized.split_once(':')?;
    Some((normalized, api_family, endpoint_kind))
}

pub(crate) fn admin_default_body_rules_for_signature(
    api_format: &str,
) -> Option<(String, Vec<serde_json::Value>)> {
    let normalized_api_format = normalize_admin_endpoint_signature(api_format)?.to_string();
    Some((normalized_api_format, Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::{admin_endpoint_signature_parts, public_api_format_local_path};

    #[test]
    fn supports_data_api_endpoint_signatures_and_public_paths() {
        for (api_format, family, kind, path) in [
            ("openai:embedding", "openai", "embedding", "/v1/embeddings"),
            (
                "gemini:interactions",
                "gemini",
                "interactions",
                "/v1/interactions",
            ),
            (
                "gemini:embedding",
                "gemini",
                "embedding",
                "/v1beta/models/{model}:{action}",
            ),
            ("openai:rerank", "openai", "rerank", "/v1/rerank"),
            ("openai:realtime", "openai", "realtime", "/v1/realtime"),
            ("openai:search", "openai", "search", "/v1/alpha/search"),
        ] {
            assert_eq!(
                admin_endpoint_signature_parts(api_format),
                Some((api_format, family, kind))
            );
            assert_eq!(public_api_format_local_path(api_format), path);
        }
    }
}
