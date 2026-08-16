use std::collections::BTreeMap;

use super::headers::{
    is_aether_internal_header, is_upstream_credential_header, normalize_upstream_accept_encoding,
    should_skip_upstream_complete_passthrough_header, should_skip_upstream_passthrough_header,
};
use super::snapshot::GatewayProviderTransportSnapshot;

const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";
const PLACEHOLDER_API_KEY: &str = "__placeholder__";

fn collect_passthrough_headers(
    headers: &http::HeaderMap,
    extra_headers: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (name, value) in headers.iter() {
        let Ok(value) = value.to_str() else {
            continue;
        };
        let key = name.as_str().to_ascii_lowercase();
        if should_skip_upstream_passthrough_header(&key) {
            continue;
        }
        let Some(value) = normalize_passthrough_header_value(&key, value) else {
            continue;
        };
        out.insert(key, value);
    }

    for (key, value) in extra_headers {
        let normalized_key = key.to_ascii_lowercase();
        if should_skip_upstream_passthrough_header(&normalized_key) {
            continue;
        }
        let Some(value) = normalize_passthrough_header_value(&normalized_key, value) else {
            continue;
        };
        out.insert(normalized_key, value);
    }

    out
}

fn collect_complete_passthrough_headers(
    headers: &http::HeaderMap,
    extra_headers: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (name, value) in headers.iter() {
        let Ok(value) = value.to_str() else {
            continue;
        };
        let key = name.as_str().to_ascii_lowercase();
        if should_skip_upstream_complete_passthrough_header(&key) {
            continue;
        }
        let Some(value) = normalize_passthrough_header_value(&key, value) else {
            continue;
        };
        out.insert(key, value);
    }

    for (key, value) in extra_headers {
        let normalized_key = key.to_ascii_lowercase();
        if should_skip_upstream_complete_passthrough_header(&normalized_key) {
            continue;
        }
        let Some(value) = normalize_passthrough_header_value(&normalized_key, value) else {
            continue;
        };
        out.insert(normalized_key, value);
    }

    out
}

fn normalize_passthrough_header_value(key: &str, value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    if key.eq_ignore_ascii_case("accept-encoding") {
        return normalize_upstream_accept_encoding(value);
    }

    Some(value.to_string())
}

pub fn build_passthrough_headers(
    headers: &http::HeaderMap,
    extra_headers: &BTreeMap<String, String>,
    content_type: Option<&str>,
) -> BTreeMap<String, String> {
    let mut out = collect_passthrough_headers(headers, extra_headers);
    out.entry("content-type".to_string()).or_insert_with(|| {
        content_type
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("application/json")
            .trim()
            .to_string()
    });
    out.remove("content-length");
    out
}

pub fn build_openai_passthrough_headers(
    headers: &http::HeaderMap,
    auth_header: &str,
    auth_value: &str,
    extra_headers: &BTreeMap<String, String>,
    content_type: Option<&str>,
) -> BTreeMap<String, String> {
    let mut out = build_passthrough_headers(headers, extra_headers, content_type);
    ensure_upstream_auth_header(&mut out, auth_header, auth_value);
    out
}

pub fn build_complete_passthrough_headers(
    headers: &http::HeaderMap,
    extra_headers: &BTreeMap<String, String>,
    content_type: Option<&str>,
) -> BTreeMap<String, String> {
    let mut out = collect_complete_passthrough_headers(headers, extra_headers);
    out.entry("content-type".to_string()).or_insert_with(|| {
        content_type
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("application/json")
            .trim()
            .to_string()
    });
    out.remove("content-length");
    out
}

pub fn build_complete_passthrough_headers_with_auth(
    headers: &http::HeaderMap,
    auth_header: &str,
    auth_value: &str,
    extra_headers: &BTreeMap<String, String>,
    content_type: Option<&str>,
) -> BTreeMap<String, String> {
    let mut out = build_complete_passthrough_headers(headers, extra_headers, content_type);
    replace_upstream_auth_headers(&mut out, auth_header, auth_value);
    out
}

pub fn build_claude_passthrough_headers(
    headers: &http::HeaderMap,
    auth_header: &str,
    auth_value: &str,
    extra_headers: &BTreeMap<String, String>,
    content_type: Option<&str>,
) -> BTreeMap<String, String> {
    let mut out = build_openai_passthrough_headers(
        headers,
        auth_header,
        auth_value,
        extra_headers,
        content_type,
    );

    for (name, value) in extra_headers {
        let key = name.to_ascii_lowercase();
        let value = value.trim();
        if value.is_empty() || !should_restore_claude_passthrough_header(&key) {
            continue;
        }

        if key == "anthropic-beta" {
            let merged = merge_comma_header_values(out.get(&key).map(String::as_str), Some(value));
            if let Some(merged) = merged {
                out.insert(key, merged);
            }
            continue;
        }

        out.insert(key, value.to_string());
    }

    for (name, value) in headers.iter() {
        let Ok(value) = value.to_str() else {
            continue;
        };
        let key = name.as_str().to_ascii_lowercase();
        let value = value.trim();
        if value.is_empty() || !should_restore_claude_passthrough_header(&key) {
            continue;
        }

        if key == "anthropic-beta" {
            let merged = merge_comma_header_values(out.get(&key).map(String::as_str), Some(value));
            if let Some(merged) = merged {
                out.insert(key, merged);
            }
            continue;
        }

        out.entry(key).or_insert_with(|| value.to_string());
    }

    out.entry("anthropic-version".to_string())
        .or_insert_with(|| DEFAULT_ANTHROPIC_VERSION.to_string());
    out
}

pub fn build_passthrough_headers_with_auth(
    headers: &http::HeaderMap,
    auth_header: &str,
    auth_value: &str,
    extra_headers: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut out = collect_passthrough_headers(headers, extra_headers);
    replace_upstream_auth_headers(&mut out, auth_header, auth_value);
    out.remove("content-length");
    out
}

pub fn ensure_upstream_auth_header(
    headers: &mut BTreeMap<String, String>,
    auth_header: &str,
    auth_value: &str,
) {
    let header_name = auth_header.trim().to_ascii_lowercase();
    let header_value = auth_value.trim();
    headers.retain(|name, _| !is_aether_internal_header(name));
    if header_name.is_empty() || header_value.is_empty() || is_aether_internal_header(&header_name)
    {
        return;
    }

    if headers
        .get(&header_name)
        .map(|value| value.trim().is_empty())
        .unwrap_or(true)
    {
        headers.insert(header_name, header_value.to_string());
    }
}

pub(crate) fn replace_upstream_auth_headers(
    headers: &mut BTreeMap<String, String>,
    auth_header: &str,
    auth_value: &str,
) {
    headers
        .retain(|name, _| !is_upstream_credential_header(name) && !is_aether_internal_header(name));
    ensure_upstream_auth_header(headers, auth_header, auth_value);
}

fn should_restore_claude_passthrough_header(name: &str) -> bool {
    name.starts_with("anthropic-") || name.starts_with("x-stainless-") || name == "x-app"
}

fn merge_comma_header_values(left: Option<&str>, right: Option<&str>) -> Option<String> {
    let mut merged = Vec::new();

    for raw in [left, right].into_iter().flatten() {
        for token in raw.split(',') {
            let token = token.trim();
            if token.is_empty() || merged.iter().any(|existing: &String| existing == token) {
                continue;
            }
            merged.push(token.to_string());
        }
    }

    if merged.is_empty() {
        None
    } else {
        Some(merged.join(","))
    }
}

pub fn resolve_local_openai_bearer_auth(
    transport: &GatewayProviderTransportSnapshot,
) -> Option<(String, String)> {
    let auth_type = resolve_local_auth_type_for_transport_format(transport);
    if !matches!(auth_type.as_str(), "api_key" | "bearer") {
        return None;
    }
    let secret = resolved_local_secret(transport)?;

    Some(("authorization".to_string(), bearer_auth_value(secret)))
}

pub fn resolve_local_standard_auth(
    transport: &GatewayProviderTransportSnapshot,
) -> Option<(String, String)> {
    let auth_type = resolve_local_auth_type_for_transport_format(transport);
    let secret = resolved_local_secret(transport)?;

    match auth_type.as_str() {
        "api_key" => Some(("x-api-key".to_string(), secret.to_string())),
        "bearer" => Some(("authorization".to_string(), bearer_auth_value(secret))),
        _ => None,
    }
}

pub fn resolve_local_gemini_auth(
    transport: &GatewayProviderTransportSnapshot,
) -> Option<(String, String)> {
    let auth_type = resolve_local_auth_type_for_transport_format(transport);
    let secret = resolved_local_secret(transport)?;

    match auth_type.as_str() {
        "api_key" => Some(("x-goog-api-key".to_string(), secret.to_string())),
        "bearer" => Some(("authorization".to_string(), bearer_auth_value(secret))),
        _ => None,
    }
}

pub fn resolve_local_auth_type_for_transport_format(
    transport: &GatewayProviderTransportSnapshot,
) -> String {
    transport.key.auth_type.trim().to_ascii_lowercase()
}

fn resolved_local_secret(transport: &GatewayProviderTransportSnapshot) -> Option<&str> {
    let secret = transport.key.decrypted_api_key.trim();
    if !secret.is_empty() && secret != PLACEHOLDER_API_KEY {
        Some(secret)
    } else {
        Some("")
    }
}

fn bearer_auth_value(secret: &str) -> String {
    if secret.is_empty() {
        String::new()
    } else {
        format!("Bearer {secret}")
    }
}
