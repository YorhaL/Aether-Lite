use std::collections::BTreeMap;

use aether_contracts::{ExecutionPlan, ExecutionResult, RequestBody};
use aether_provider_transport::auth::{
    ensure_upstream_auth_header, resolve_local_gemini_auth, resolve_local_openai_bearer_auth,
    resolve_local_standard_auth,
};
use aether_provider_transport::{
    apply_local_header_rules, resolve_transport_execution_timeouts,
    GatewayProviderTransportSnapshot,
};
use async_trait::async_trait;
use serde_json::json;

use crate::build_models_fetch_url;

#[async_trait]
pub trait ModelFetchTransportRuntime: Send + Sync {
    async fn execute_model_fetch_execution_plan(
        &self,
        plan: &ExecutionPlan,
    ) -> Result<ExecutionResult, String>;
}

pub fn build_standard_models_fetch_execution_plan(
    transport: &GatewayProviderTransportSnapshot,
    after_id: Option<&str>,
) -> Result<ExecutionPlan, String> {
    let api_format =
        aether_ai_formats::normalize_api_format_alias(transport.endpoint.api_format.trim());
    let mut headers = BTreeMap::from([("accept".to_string(), "application/json".to_string())]);
    let mut protected_headers = Vec::new();
    let mut url = build_models_fetch_url(&api_format, &transport.endpoint.base_url)
        .map(|(url, _)| url)
        .ok_or_else(|| "models fetch is unavailable for this endpoint format".to_string())?;

    match api_format.as_str() {
        format if format.starts_with("openai:") => {
            apply_header_auth(
                &mut headers,
                &mut protected_headers,
                resolve_local_openai_bearer_auth(transport),
            )?;
        }
        format if format.starts_with("claude:") => {
            headers.insert("anthropic-version".to_string(), "2023-06-01".to_string());
            apply_header_auth(
                &mut headers,
                &mut protected_headers,
                resolve_local_standard_auth(transport),
            )?;
            url = append_query_param(url, "limit", "100");
            if let Some(after_id) = after_id.map(str::trim).filter(|value| !value.is_empty()) {
                url = append_query_param(url, "after_id", after_id);
            }
        }
        format if format.starts_with("gemini:") => {
            let auth = resolve_local_gemini_auth(transport)
                .ok_or_else(|| "models fetch authentication is unavailable".to_string())?;
            if auth.0.eq_ignore_ascii_case("x-goog-api-key") {
                url = append_query_param(url, "key", auth.1.trim());
            } else {
                apply_header_auth(&mut headers, &mut protected_headers, Some(auth))?;
            }
        }
        _ => {}
    }

    let protected = protected_headers
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    if !apply_local_header_rules(
        &mut headers,
        transport.endpoint.header_rules.as_ref(),
        &protected,
        &json!({}),
        None,
    ) {
        return Err("endpoint header rules could not be applied".to_string());
    }

    Ok(ExecutionPlan {
        request_id: format!("req-model-fetch-{}", transport.key.id),
        candidate_id: None,
        provider_name: Some(transport.provider.name.clone()),
        provider_id: transport.provider.id.clone(),
        endpoint_id: transport.endpoint.id.clone(),
        key_id: transport.key.id.clone(),
        method: "GET".to_string(),
        url,
        headers,
        content_type: None,
        content_encoding: None,
        body: RequestBody {
            json_body: None,
            body_bytes_b64: None,
            body_ref: None,
        },
        stream: false,
        client_api_format: api_format.clone(),
        provider_api_format: api_format,
        model_name: Some("models".to_string()),
        transport_profile: None,
        timeouts: resolve_transport_execution_timeouts(transport),
    })
}

fn apply_header_auth(
    headers: &mut BTreeMap<String, String>,
    protected_headers: &mut Vec<String>,
    auth: Option<(String, String)>,
) -> Result<(), String> {
    let Some((name, value)) = auth else {
        return Err("models fetch authentication is unavailable".to_string());
    };
    if !name.trim().is_empty() && !value.trim().is_empty() {
        protected_headers.push(name.clone());
        ensure_upstream_auth_header(headers, &name, &value);
    }
    Ok(())
}

fn append_query_param(mut url: String, key: &str, value: &str) -> String {
    if key.trim().is_empty() || value.trim().is_empty() {
        return url;
    }
    let separator = if url.contains('?') { '&' } else { '?' };
    url.push(separator);
    url.push_str(key.trim());
    url.push('=');
    url.push_str(value.trim());
    url
}
