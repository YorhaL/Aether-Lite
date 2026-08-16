use std::collections::BTreeMap;
use std::sync::OnceLock;

use aether_ai_formats::ApiOperation;
use regex::Regex;
use serde_json::Value;
use url::form_urlencoded;

use crate::snapshot::GatewayProviderTransportSnapshot;
use crate::url::{
    build_claude_count_tokens_url, build_claude_messages_url, build_gemini_content_url,
    build_openai_chat_url, build_openai_responses_url, build_openai_search_url,
    build_passthrough_path_url, normalize_gemini_content_action_path,
    strip_gateway_credential_query_parameters, GATEWAY_CREDENTIAL_QUERY_KEYS,
};

#[derive(Debug, Clone, Copy)]
pub struct TransportRequestUrlParams<'a> {
    pub provider_api_format: &'a str,
    pub mapped_model: Option<&'a str>,
    pub upstream_is_stream: bool,
    pub request_query: Option<&'a str>,
    pub api_operation: Option<ApiOperation>,
}

pub fn build_transport_request_url(
    transport: &GatewayProviderTransportSnapshot,
    params: TransportRequestUrlParams<'_>,
) -> Option<String> {
    build_transport_request_url_inner(transport, params, false)
}

pub fn build_transport_request_url_for_request_body(
    transport: &GatewayProviderTransportSnapshot,
    params: TransportRequestUrlParams<'_>,
    provider_request_body: Option<&Value>,
) -> Option<String> {
    let gemini_embedding_batch =
        gemini_embedding_request_body_uses_batch(params.provider_api_format, provider_request_body);
    build_transport_request_url_inner(transport, params, gemini_embedding_batch)
}

fn gemini_embedding_request_body_uses_batch(
    provider_api_format: &str,
    provider_request_body: Option<&Value>,
) -> bool {
    aether_ai_formats::normalize_api_format_alias(provider_api_format) == "gemini:embedding"
        && provider_request_body
            .and_then(|body| body.get("requests"))
            .and_then(Value::as_array)
            .is_some_and(|requests| !requests.is_empty())
}

fn build_transport_request_url_inner(
    transport: &GatewayProviderTransportSnapshot,
    params: TransportRequestUrlParams<'_>,
    gemini_embedding_batch: bool,
) -> Option<String> {
    let provider_api_format =
        aether_ai_formats::normalize_api_format_alias(params.provider_api_format);
    if !transport_supports_api_operation(&provider_api_format, params.api_operation) {
        return None;
    }

    let sanitized_query = if provider_api_format == "claude:messages" {
        strip_gateway_credential_query_parameters(params.request_query)
    } else {
        params.request_query.map(ToOwned::to_owned)
    };
    let params = TransportRequestUrlParams {
        request_query: sanitized_query.as_deref(),
        ..params
    };

    if let Some(path) = transport
        .endpoint
        .custom_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        return build_custom_path_url(
            transport,
            params,
            &provider_api_format,
            path,
            gemini_embedding_batch,
        );
    }

    let url = match provider_api_format.as_str() {
        "openai:chat" => Some(build_openai_chat_url(
            &transport.endpoint.base_url,
            params.request_query,
        )),
        "openai:responses" => Some(build_openai_responses_url(
            &transport.endpoint.base_url,
            params.request_query,
            false,
        )),
        "openai:responses:compact" => Some(build_openai_responses_url(
            &transport.endpoint.base_url,
            params.request_query,
            true,
        )),
        "openai:search" => Some(build_openai_search_url(
            &transport.endpoint.base_url,
            params.request_query,
        )),
        "openai:embedding" | "jina:embedding" => build_provider_api_root_url(
            &transport.endpoint.base_url,
            "/embeddings",
            params.request_query,
        ),
        "openai:rerank" | "jina:rerank" => build_provider_api_root_url(
            &transport.endpoint.base_url,
            "/rerank",
            params.request_query,
        ),
        "aliyun:multimodal_embedding" => build_passthrough_path_url(
            &transport.endpoint.base_url,
            "/api/v1/services/embeddings/multimodal-embedding/multimodal-embedding",
            params.request_query,
            &[],
        ),
        "doubao:embedding" => build_passthrough_path_url(
            &transport.endpoint.base_url,
            "/embeddings",
            params.request_query,
            &[],
        ),
        "claude:messages" => Some(
            if params.api_operation == Some(ApiOperation::ClaudeCountTokens) {
                build_claude_count_tokens_url(&transport.endpoint.base_url, params.request_query)
            } else {
                build_claude_messages_url(&transport.endpoint.base_url, params.request_query)
            },
        ),
        "gemini:generate_content" => build_gemini_content_url(
            &transport.endpoint.base_url,
            params.mapped_model?,
            params.upstream_is_stream,
            params.request_query,
        ),
        "gemini:embedding" => build_gemini_embedding_url(
            &transport.endpoint.base_url,
            params.mapped_model?,
            params.request_query,
            gemini_embedding_batch,
        ),
        "gemini:interactions" => build_passthrough_path_url(
            &transport.endpoint.base_url,
            "/v1/interactions",
            params.request_query,
            &["key"],
        ),
        _ => None,
    }?;

    Some(add_gemini_stream_query(
        url,
        &provider_api_format,
        params.upstream_is_stream,
    ))
}

fn build_custom_path_url(
    transport: &GatewayProviderTransportSnapshot,
    params: TransportRequestUrlParams<'_>,
    provider_api_format: &str,
    path_template: &str,
    gemini_embedding_batch: bool,
) -> Option<String> {
    let handles_operation = path_template.contains("{operation}");
    let expanded = expand_custom_path_template(
        path_template,
        build_path_params(params, provider_api_format, gemini_embedding_batch),
    );
    let path = if provider_api_format == "gemini:generate_content" {
        normalize_gemini_content_action_path(&expanded, params.upstream_is_stream)
    } else if provider_api_format == "gemini:embedding" {
        normalize_gemini_embedding_action_path(&expanded, gemini_embedding_batch)
    } else {
        expanded
    };
    let blocked_query_keys =
        if provider_api_format.starts_with("gemini:") || provider_api_format == "claude:messages" {
            GATEWAY_CREDENTIAL_QUERY_KEYS
        } else {
            &[]
        };
    let mut url = build_passthrough_path_url(
        &transport.endpoint.base_url,
        &path,
        params.request_query,
        blocked_query_keys,
    )?;
    if params.api_operation == Some(ApiOperation::ClaudeCountTokens) && !handles_operation {
        url = build_claude_count_tokens_url(&url, None);
    }
    Some(add_gemini_stream_query(
        url,
        provider_api_format,
        params.upstream_is_stream,
    ))
}

fn build_path_params<'a>(
    params: TransportRequestUrlParams<'a>,
    provider_api_format: &str,
    gemini_embedding_batch: bool,
) -> BTreeMap<&'static str, &'a str> {
    let mut path_params = BTreeMap::new();
    if let Some(model) = params
        .mapped_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        path_params.insert("model", model);
    }
    if let Some(operation) = params.api_operation {
        path_params.insert("operation", operation.as_str());
    }
    if provider_api_format == "gemini:generate_content" || provider_api_format == "gemini:embedding"
    {
        let action = if provider_api_format == "gemini:embedding" {
            if gemini_embedding_batch {
                "batchEmbedContents"
            } else {
                "embedContent"
            }
        } else if params.upstream_is_stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        path_params.insert("action", action);
    }
    path_params
}

fn transport_supports_api_operation(
    provider_api_format: &str,
    operation: Option<ApiOperation>,
) -> bool {
    operation != Some(ApiOperation::ClaudeCountTokens) || provider_api_format == "claude:messages"
}

fn build_provider_api_root_url(
    upstream_base_url: &str,
    path: &str,
    query: Option<&str>,
) -> Option<String> {
    build_passthrough_path_url(upstream_base_url, path, query, &[])
}

fn build_gemini_embedding_url(
    upstream_base_url: &str,
    model: &str,
    query: Option<&str>,
    batch: bool,
) -> Option<String> {
    let trimmed_base_url = upstream_base_url
        .trim()
        .split_once('?')
        .map(|(base, _)| base)
        .unwrap_or_else(|| upstream_base_url.trim())
        .trim_end_matches('/');
    let model = model.trim();
    if trimmed_base_url.is_empty() || model.is_empty() {
        return None;
    }
    let action = if batch {
        "batchEmbedContents"
    } else {
        "embedContent"
    };
    let path = if trimmed_base_url.ends_with("/v1beta") {
        format!("/models/{model}:{action}")
    } else if trimmed_base_url.contains("/v1beta/models/") {
        format!(":{action}")
    } else {
        format!("/v1beta/models/{model}:{action}")
    };
    build_passthrough_path_url(upstream_base_url, &path, query, &["key"])
}

fn normalize_gemini_embedding_action_path(path: &str, batch: bool) -> String {
    if batch {
        path.replace(":embedContent", ":batchEmbedContents")
    } else {
        path.replace(":batchEmbedContents", ":embedContent")
    }
}

fn expand_custom_path_template(path: &str, params: BTreeMap<&'static str, &str>) -> String {
    if params.is_empty() {
        return path.to_string();
    }
    let mut missing_key = false;
    let replaced =
        custom_path_template_regex().replace_all(path, |captures: &regex::Captures<'_>| {
            let key = captures
                .get(1)
                .map(|value| value.as_str())
                .unwrap_or_default();
            match params.get(key).copied() {
                Some(value) => value.to_string(),
                None => {
                    missing_key = true;
                    captures
                        .get(0)
                        .map(|value| value.as_str().to_string())
                        .unwrap_or_default()
                }
            }
        });
    if missing_key {
        path.to_string()
    } else {
        replaced.into_owned()
    }
}

fn add_gemini_stream_query(
    upstream_url: String,
    provider_api_format: &str,
    upstream_is_stream: bool,
) -> String {
    if provider_api_format != "gemini:generate_content" || !upstream_is_stream {
        return upstream_url;
    }
    let has_alt = upstream_url
        .split_once('?')
        .map(|(_, query)| {
            form_urlencoded::parse(query.as_bytes()).any(|(key, _)| key.eq_ignore_ascii_case("alt"))
        })
        .unwrap_or(false);
    if has_alt {
        upstream_url
    } else if upstream_url.contains('?') {
        format!("{upstream_url}&alt=sse")
    } else {
        format!("{upstream_url}?alt=sse")
    }
}

fn custom_path_template_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"\{([A-Za-z_][A-Za-z0-9_]*)\}")
            .expect("custom path template regex must compile")
    })
}
